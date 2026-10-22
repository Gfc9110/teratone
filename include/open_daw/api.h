#ifndef OPEN_DAW_API_H
#define OPEN_DAW_API_H
#include <stddef.h>
#include <stdint.h>
#if defined(_WIN32) && defined(OD_SHARED)
# if defined(OD_BUILDING)
#  define OD_API __declspec(dllexport)
# else
#  define OD_API __declspec(dllimport)
# endif
#elif defined(OD_SHARED)
# define OD_API __attribute__((visibility("default")))
#else
# define OD_API
#endif
#ifdef __cplusplus
extern "C" {
#define OD_NOEXCEPT noexcept
#else
#define OD_NOEXCEPT
#endif
#define OD_ABI_VERSION 1u
#define OD_LIVE_ID_BIT (UINT64_C(1) << 63)
typedef struct od_engine od_engine;
typedef enum od_result { OD_OK=0, OD_INVALID=1, OD_QUEUE_FULL=2, OD_BUSY=3, OD_ERROR=4 } od_result;
typedef struct od_note {
    uint64_t id, start_frame, end_frame;
    uint32_t track, pitch, velocity;
} od_note;
typedef struct od_track {
    float gain, pan;
    const char* sample_path_utf8; /* NULL/empty = sine; copied synchronously */
    uint32_t root_note, one_shot;
} od_track;
/* Additive descriptors: the original od_track / od_load layout remains unchanged. */
#define OD_TRACK_V2_VERSION 2u
typedef struct od_track_v2 {
    uint32_t struct_size, version;
    od_track base;
    uint32_t mute, solo;
} od_track_v2;
typedef struct od_track_mix_v1 { float gain, pan; uint32_t mute, solo; } od_track_mix_v1;
typedef struct od_mix_status_v1 { uint64_t generation, submitted, applied; } od_mix_status_v1;
typedef struct od_telemetry {
    uint64_t position, rendered_frames, blocks, last_applied, rejected_commands, clipped_samples;
    float peak_left, peak_right;
    uint32_t playing, looping, device_open, device_error;
} od_telemetry;
/* All calls for a handle on ONE control thread. Device owns the audio consumer.
 * Host must provide valid pointers/lengths; this is a trusted in-process ABI, not a sandbox.
 * Returned error text is thread-local and valid until the next API call on that thread.
 * load validates/prepares first, then stops/joins output and replaces state. On validation
 * failure the old session/device is retained. Successful load leaves device CLOSED.
 * render is headless only and returns OD_BUSY while a device is open.
 */
OD_API uint32_t od_abi_version(void) OD_NOEXCEPT;
OD_API const char* od_error_message(void) OD_NOEXCEPT;
OD_API od_engine* od_create(uint32_t sample_rate, uint32_t max_frames) OD_NOEXCEPT;
OD_API void od_destroy(od_engine*) OD_NOEXCEPT;
OD_API od_result od_load(od_engine*, const od_track*, size_t track_count,
                         const od_note*, size_t note_count, uint64_t length_frames) OD_NOEXCEPT;
/* v2 load sets the initial gate with no audible startup ramp/leak. */
OD_API od_result od_load_v2(od_engine*, const od_track_v2*, size_t track_count,
                            const od_note*, size_t note_count, uint64_t length_frames) OD_NOEXCEPT;
/* Complete mixer snapshot, copied synchronously, applied coherently at a block boundary.
 * OD_QUEUE_FULL changes neither the accepted/exportable state nor out_serial.
 * Generation guards slots across successful loads; serial acknowledges acceptance only. */
OD_API od_result od_set_mix_v1(od_engine*, uint64_t generation, const od_track_mix_v1*,
                              size_t track_count, uint64_t* out_serial) OD_NOEXCEPT;
OD_API od_result od_read_mix_status_v1(const od_engine*, od_mix_status_v1*) OD_NOEXCEPT;
OD_API od_result od_play(od_engine*) OD_NOEXCEPT;
OD_API od_result od_pause(od_engine*) OD_NOEXCEPT;
OD_API od_result od_seek(od_engine*, uint64_t frame) OD_NOEXCEPT;
OD_API od_result od_loop(od_engine*, uint32_t enabled, uint64_t begin, uint64_t end) OD_NOEXCEPT;
OD_API od_result od_live_note(od_engine*, uint32_t track, uint64_t id, uint32_t pitch, uint32_t velocity, uint32_t on) OD_NOEXCEPT;
OD_API od_result od_stop(od_engine*) OD_NOEXCEPT;
OD_API od_result od_panic(od_engine*) OD_NOEXCEPT;
/* Legacy wrapper submits a complete mixer snapshot; OD_QUEUE_FULL requires retry. */
OD_API od_result od_gain_pan(od_engine*, uint32_t track, float gain, float pan) OD_NOEXCEPT;
OD_API od_result od_read_telemetry(const od_engine*, od_telemetry*) OD_NOEXCEPT;
OD_API od_result od_render(od_engine*, float* stereo, uint32_t frames) OD_NOEXCEPT;
OD_API od_result od_device_start(od_engine*, uint32_t silent_clock) OD_NOEXCEPT;
OD_API od_result od_device_stop(od_engine*) OD_NOEXCEPT;
OD_API od_result od_export(od_engine*, const char* path_utf8, uint64_t frames) OD_NOEXCEPT;
#ifdef __cplusplus
}
#endif
#endif
