// The Oboe shim.
//
// Oboe is a C++ library with a callback-based API. The shim keeps a ring of
// samples that the Rust side writes into and that the audio callback drains.
//
// The callback runs on a real-time thread, so it must not lock, allocate or
// call into the JVM. It only touches the ring buffer.

#include "vimo_oboe.h"

#include <atomic>
#include <cstring>
#include <mutex>
#include <vector>

#include <oboe/Oboe.h>

namespace {

constexpr int32_t kRingFrames = 8192;

struct VimoAudio {
  std::shared_ptr<oboe::AudioStream> stream;
  std::mutex mutex;
  std::vector<float> ring;
  std::atomic<int32_t> write_index{0};
  std::atomic<int32_t> read_index{0};
  int32_t channels = 2;
  int32_t sample_rate = 48000;
  bool running = false;
};

class VimoCallback : public oboe::AudioStreamDataCallback {
 public:
  explicit VimoCallback(VimoAudio *owner) : owner_(owner) {}

  std::atomic<int64_t> played_frames{0};

  void onAudioReady(oboe::AudioStream *stream, void *audio_data,
                    int32_t num_frames) override {
    auto *out = static_cast<float *>(audio_data);
    const int32_t channels = owner_->channels;
    const int32_t mask = kRingFrames - 1;

    int32_t write = owner_->write_index.load(std::memory_order_acquire);
    int32_t read = owner_->read_index.load(std::memory_order_relaxed);

    for (int32_t i = 0; i < num_frames; ++i) {
      const int32_t slot = (read + i) & mask;
      for (int32_t c = 0; c < channels; ++c) {
        const int32_t index = (write + i * channels + c) & mask;
        // The ring is indexed by frame; the sample layout is interleaved.
        const int32_t source = (slot * channels + c) & (kRingFrames * channels - 1);
        out[i * channels + c] = owner_->ring[source];
      }
    }

    owner_->read_index.store((read + num_frames) & mask, std::memory_order_release);
    played_frames.fetch_add(num_frames, std::memory_order_relaxed);
  }

  void onErrorAfterClose(oboe::AudioStream *stream,
                         oboe::Result error) override {}

 private:
  VimoAudio *owner_;
};

}  // namespace

extern "C" {

VimoAudio *vimo_audio_create(int32_t sample_rate, int32_t channels,
                             int32_t frames_per_callback) {
  if (channels < 1) {
    channels = 2;
  }
  if (frames_per_callback < 32) {
    frames_per_callback = 128;
  }

  auto *audio = new VimoAudio();
  audio->channels = channels;
  audio->sample_rate = sample_rate;
  audio->ring.assign(static_cast<size_t>(kRingFrames * channels), 0.0f);

  oboe::AudioStreamBuilder builder;
  builder.setDirection(oboe::Direction::Output)
      ->setSharingMode(oboe::SharingMode::Exclusive)
      ->setFormat(oboe::Format::Float)
      ->setChannelCount(channels)
      ->setSampleRate(sample_rate)
      ->setPerformanceMode(oboe::PerformanceMode::LowLatency)
      ->setUsage(oboe::Usage::Media)
      ->setDataCallback(new VimoCallback(audio));

  std::shared_ptr<oboe::AudioStream> stream;
  oboe::Result result = builder.openStream(stream);
  if (result != oboe::Result::OK) {
    // Fall back to a shared stream, which is more widely supported.
    builder.setSharingMode(oboe::SharingMode::Shared);
    result = builder.openStream(stream);
  }
  if (result != oboe::Result::OK || stream == nullptr) {
    delete audio;
    return nullptr;
  }

  audio->stream = stream;
  return audio;
}

void vimo_audio_destroy(VimoAudio *audio) {
  if (audio == nullptr) {
    return;
  }
  if (audio->stream) {
    audio->stream->stop();
    audio->stream->close();
  }
  delete audio;
}

int32_t vimo_audio_start(VimoAudio *audio) {
  if (audio == nullptr || !audio->stream) {
    return -1;
  }
  if (audio->running) {
    return 0;
  }
  const oboe::Result result = audio->stream->start();
  audio->running = result == oboe::Result::OK;
  return audio->running ? 0 : static_cast<int32_t>(result);
}

void vimo_audio_stop(VimoAudio *audio) {
  if (audio == nullptr || !audio->stream) {
    return;
  }
  audio->stream->stop();
  audio->running = false;
}

int32_t vimo_audio_write(VimoAudio *audio, const float *data, int32_t frames) {
  if (audio == nullptr || data == nullptr || frames <= 0) {
    return 0;
  }
  const int32_t channels = audio->channels;
  const int32_t mask = kRingFrames - 1;
  const int32_t sample_mask = (kRingFrames * channels) - 1;

  std::lock_guard<std::mutex> guard(audio->mutex);
  int32_t write = audio->write_index.load(std::memory_order_relaxed);
  const int32_t read = audio->read_index.load(std::memory_order_acquire);

  // Only write what fits without overrunning the reader.
  int32_t free_frames = kRingFrames - ((write - read) & mask);
  if (free_frames <= 0) {
    return 0;
  }
  if (frames > free_frames) {
    frames = free_frames;
  }
  for (int32_t i = 0; i < frames * channels; ++i) {
    audio->ring[(write * channels + i) & sample_mask] = data[i];
  }
  audio->write_index.store((write + frames) & mask, std::memory_order_release);
  return frames;
}

int32_t vimo_audio_latency_frames(VimoAudio *audio) {
  if (audio == nullptr || !audio->stream) {
    return 0;
  }
  return audio->stream->getLatencyInFrames();
}

int64_t vimo_audio_position_frames(VimoAudio *audio) {
  if (audio == nullptr || !audio->stream) {
    return 0;
  }
  return audio->stream->getFramesWritten();
}

}  // extern "C"
