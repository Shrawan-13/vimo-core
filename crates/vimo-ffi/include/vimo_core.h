/* vimo_core.h — the C ABI of the Vimo Core.
 *
 * The Android layer talks to the core exclusively through this interface.
 * The rules are:
 *
 *   1. The engine is an opaque handle. Nothing about the Rust types leaks.
 *   2. Commands go in, events come out. The UI never queries the engine;
 *      it asks for a snapshot when it needs to redraw the timeline.
 *   3. Every entry point is panic proof: a Rust panic becomes a status code.
 *   4. Frames are rendered on the engine's own thread into a surface that was
 *      handed over once. There is no per-frame call across this boundary.
 *
 * All functions are safe to call from any thread except that a single handle
 * must not be destroyed while another thread is using it.
 */

#ifndef VIMO_CORE_H
#define VIMO_CORE_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ------------------------------------------------------------------ */
/* Status codes                                                        */
/* ------------------------------------------------------------------ */

#define VIMO_OK 0
#define VIMO_INVALID_HANDLE (-1)
#define VIMO_INVALID_ARGUMENT (-2)
#define VIMO_ERROR (-3)
#define VIMO_PANIC (-4)
#define VIMO_UNSUPPORTED (-5)

/* ------------------------------------------------------------------ */
/* Command tags. Never renumber an existing tag.                       */
/* ------------------------------------------------------------------ */

enum {
    /* Playback. */
    VIMO_CMD_PLAY = 1,
    VIMO_CMD_PAUSE = 2,
    VIMO_CMD_STOP = 3,
    VIMO_CMD_SEEK = 4,
    VIMO_CMD_SET_RATE = 5,
    VIMO_CMD_UNDO = 6,
    VIMO_CMD_REDO = 7,

    /* Editing. */
    VIMO_CMD_ADD_CLIP = 8,
    VIMO_CMD_REMOVE_CLIP = 9,
    VIMO_CMD_SET_PARAM = 10,

    /* Files. */
    VIMO_CMD_LOAD_PROJECT = 11,
    VIMO_CMD_SAVE_PROJECT = 12,

    /* Export. */
    VIMO_CMD_START_EXPORT = 13,
    VIMO_CMD_CANCEL_EXPORT = 14,

    /* Platform. */
    VIMO_CMD_SET_SURFACE = 15,
    VIMO_CMD_THERMAL = 16,
    VIMO_CMD_MEMORY = 17,
    VIMO_CMD_SET_PREVIEW_SCALE = 18,

    /* Clips. */
    VIMO_CMD_MOVE_CLIP = 19,
    VIMO_CMD_TRIM_CLIP = 20,
    VIMO_CMD_SPLIT_CLIP = 21,
    VIMO_CMD_SET_CLIP_ENABLED = 22,
    VIMO_CMD_SET_CLIP_OPACITY = 23,
    VIMO_CMD_SET_CLIP_BLEND = 24,
    VIMO_CMD_SET_CLIP_TRANSFORM = 25,
    VIMO_CMD_SET_CLIP_VOLUME = 26,
    VIMO_CMD_SET_CLIP_SPEED = 27,
    VIMO_CMD_SET_CLIP_MUTED = 28,

    /* Tracks. */
    VIMO_CMD_ADD_TRACK = 29,
    VIMO_CMD_REMOVE_TRACK = 30,
    VIMO_CMD_SET_TRACK_ENABLED = 31,
    VIMO_CMD_SET_TRACK_MUTED = 32,
    VIMO_CMD_SET_TRACK_SOLO = 33,
    VIMO_CMD_SET_TRACK_LOCKED = 34,
    VIMO_CMD_SET_TRACK_VOLUME = 35,

    /* Effects. */
    VIMO_CMD_ADD_EFFECT = 36,
    VIMO_CMD_REMOVE_EFFECT = 37,
    VIMO_CMD_SET_EFFECT_ENABLED = 38,
    VIMO_CMD_ADD_KEYFRAME = 39,
    VIMO_CMD_REMOVE_KEYFRAME = 40,
    VIMO_CMD_SET_KEYFRAME_INTERPOLATION = 41,

    /* Assets and transitions. */
    VIMO_CMD_ADD_ASSET = 42,
    VIMO_CMD_REMOVE_ASSET = 43,
    VIMO_CMD_ADD_TRANSITION = 44,
    VIMO_CMD_REMOVE_TRANSITION = 45,

    /* Project settings. */
    VIMO_CMD_SET_PROJECT_NAME = 46,
    VIMO_CMD_SET_RESOLUTION = 47,
    VIMO_CMD_SET_FPS = 48,
    VIMO_CMD_SET_AUTOSAVE_DIR = 49,
    VIMO_CMD_RECOVER_AUTOSAVE = 50,

    /* Playback extras. */
    VIMO_CMD_SET_LOOP = 51,
    VIMO_CMD_EXPORT_PAUSE = 52,
    VIMO_CMD_EXPORT_RESUME = 53,
    VIMO_CMD_REQUEST_SNAPSHOT = 54
};

/* ------------------------------------------------------------------ */
/* Event tags. Never renumber an existing tag.                         */
/* ------------------------------------------------------------------ */

enum {
    VIMO_EVENT_PROJECT_CHANGED = 1,
    VIMO_EVENT_PLAYBACK_STATE = 2,
    VIMO_EVENT_POSITION = 3,
    VIMO_EVENT_FRAME_RENDERED = 4,
    VIMO_EVENT_MEMORY_PRESSURE = 5,
    VIMO_EVENT_THERMAL = 6,
    VIMO_EVENT_EXPORT_PROGRESS = 7,
    VIMO_EVENT_EXPORT_FINISHED = 8,
    VIMO_EVENT_ERROR = 9,
    VIMO_EVENT_SNAPSHOT = 10
};

/* Playback states. */
enum {
    VIMO_PLAYBACK_STOPPED = 0,
    VIMO_PLAYBACK_PLAYING = 1,
    VIMO_PLAYBACK_PAUSED = 2
};

/* Memory pressure levels. */
enum {
    VIMO_MEMORY_NORMAL = 0,
    VIMO_MEMORY_WARNING = 1,
    VIMO_MEMORY_CRITICAL = 2
};

/* Thermal states (mirrors Android's THERMAL_STATUS_*). */
enum {
    VIMO_THERMAL_NOMINAL = 0,
    VIMO_THERMAL_LIGHT = 1,
    VIMO_THERMAL_MODERATE = 2,
    VIMO_THERMAL_SEVERE = 3,
    VIMO_THERMAL_CRITICAL = 4,
    VIMO_THERMAL_EMERGENCY = 5
};

/* Blend modes. */
enum {
    VIMO_BLEND_NORMAL = 0,
    VIMO_BLEND_ADD = 1,
    VIMO_BLEND_SCREEN = 2,
    VIMO_BLEND_MULTIPLY = 3,
    VIMO_BLEND_OVERLAY = 4,
    VIMO_BLEND_HARD_LIGHT = 5,
    VIMO_BLEND_SOFT_LIGHT = 6,
    VIMO_BLEND_LIGHTEN = 7,
    VIMO_BLEND_DARKEN = 8,
    VIMO_BLEND_COLOR_DODGE = 9,
    VIMO_BLEND_COLOR_BURN = 10,
    VIMO_BLEND_LINEAR_DODGE = 11,
    VIMO_BLEND_LINEAR_BURN = 12,
    VIMO_BLEND_DIFFERENCE = 13,
    VIMO_BLEND_EXCLUSION = 14,
    VIMO_BLEND_HUE = 15,
    VIMO_BLEND_SATURATION = 16,
    VIMO_BLEND_COLOR = 17,
    VIMO_BLEND_LUMINOSITY = 18
};

/* Effect owner kinds. */
enum {
    VIMO_OWNER_CLIP = 0,
    VIMO_OWNER_TRACK = 1
};

/* Keyframe interpolation kinds. */
enum {
    VIMO_INTERP_LINEAR = 0,
    VIMO_INTERP_HOLD = 1,
    VIMO_INTERP_BEZIER = 2
};

/* ------------------------------------------------------------------ */
/* Structures                                                          */
/* ------------------------------------------------------------------ */

typedef struct VimoEngine VimoEngine;

typedef struct {
    uint32_t width;
    uint32_t height;
    uint32_t sample_rate;
    uint16_t channels;
    uint32_t flags; /* reserved, must be 0 */
} VimoConfig;

typedef struct {
    uint32_t tag;   /* one of VIMO_CMD_* */
    int64_t a;
    int64_t b;
    int64_t c;
    const uint8_t *text; /* optional, not NUL terminated */
    uint32_t text_len;
    const float *floats; /* optional parameter array */
    uint32_t float_count;
} VimoCommand;

typedef struct {
    uint32_t tag; /* one of VIMO_EVENT_* */
    int64_t a;
    int64_t b;
    int32_t code;
    const uint8_t *message; /* valid until the next poll on this thread */
    uint32_t message_len;
} VimoEvent;

typedef struct {
    uint64_t frames_rendered;
    uint64_t frames_dropped;
    uint64_t frames_held;
    uint64_t cache_hits;
    uint64_t cache_misses;
    uint64_t cache_bytes;
    uint32_t decoders_live;
} VimoStats;

/* ------------------------------------------------------------------ */
/* Entry points                                                        */
/* ------------------------------------------------------------------ */

/* The ABI version. The Java layer checks it against its own constant. */
uint32_t vimo_abi_version(void);

/* Create an engine. Returns NULL on failure. */
VimoEngine *vimo_engine_create(const VimoConfig *config);

/* Destroy an engine. */
int32_t vimo_engine_destroy(VimoEngine *engine);

/* Execute a command. Returns a VIMO_* status code. */
int32_t vimo_engine_execute(VimoEngine *engine, const VimoCommand *command);

/* Poll for events. Returns the number of events written, or a negative
 * status code. */
int32_t vimo_engine_poll_events(VimoEngine *engine, VimoEvent *out, int32_t max_events);

/* Read the engine statistics. */
int32_t vimo_engine_stats(VimoEngine *engine, VimoStats *out);

/* Write a JSON snapshot of the project into `out`.
 *
 * Returns the number of bytes written. When the buffer is too small the
 * required size is returned instead, so the caller can allocate and retry.
 * A negative value is a status code. */
int32_t vimo_engine_snapshot(VimoEngine *engine, uint8_t *out, int32_t capacity);

/* The last error message for an engine. Valid until the next call on the
 * same thread. */
const char *vimo_engine_last_error(VimoEngine *engine);

#ifdef __cplusplus
} /* extern "C" */
#endif

#endif /* VIMO_CORE_H */
