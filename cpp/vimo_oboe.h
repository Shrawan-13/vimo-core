/*
 * vimo_oboe.h — the C interface to the Oboe audio output.
 *
 * Oboe is a C++ library. The Rust platform layer cannot call it directly, so
 * this small shim exposes it as plain C: the JNI and Rust sides stay free of
 * C++ and the audio callback gets a simple C entry point.
 */

#ifndef VIMO_OBOE_H
#define VIMO_OBOE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/** The opaque audio stream. */
typedef struct VimoAudio VimoAudio;

/**
 * Create an output stream.
 *
 * @param sample_rate the sample rate in Hz (48000 is the usual choice)
 * @param channels the number of interleaved channels (1 or 2)
 * @param frames_per_callback the number of frames per callback (128-256 for
 *        low latency)
 * @return the stream, or NULL on failure
 */
VimoAudio *vimo_audio_create(int32_t sample_rate, int32_t channels,
                             int32_t frames_per_callback);

/** Release the stream. */
void vimo_audio_destroy(VimoAudio *audio);

/**
 * Start the stream.
 *
 * @return 0 on success, non-zero on failure
 */
int32_t vimo_audio_start(VimoAudio *audio);

/** Stop the stream. */
void vimo_audio_stop(VimoAudio *audio);

/**
 * Queue samples for playback.
 *
 * @param data the interleaved samples
 * @param frames the number of sample frames
 * @return the number of frames accepted, or a negative value on failure
 */
int32_t vimo_audio_write(VimoAudio *audio, const float *data, int32_t frames);

/** The output latency, in frames. */
int32_t vimo_audio_latency_frames(VimoAudio *audio);

/** How far the device has played, in frames. */
int64_t vimo_audio_position_frames(VimoAudio *audio);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* VIMO_OBOE_H */
