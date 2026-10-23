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
#define OD_MAX_TRACKS 64u
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
#define OD_SYNTH_V1_VERSION 1u
/* waveform: 0=sine, 1=saw, 2=square/PWM, 3=triangle; seconds for A/D/R.
 * shape [0,1], pulse_width [.05,.95], A/D [0,10], S [0,1], R [0,20]. */
typedef struct od_synth_v1 {
    uint32_t struct_size, version, waveform;
    float shape, pulse_width, attack, decay, sustain, release;
} od_synth_v1;
#define OD_TRACK_V3_VERSION 3u
typedef struct od_track_v3 {
    uint32_t struct_size, version;
    od_track_v2 base;
    od_synth_v1 synth; /* validated also for sampler slots; ignored by sampler DSP */
} od_track_v3;
typedef struct od_track_mix_v1 { float gain, pan; uint32_t mute, solo; } od_track_mix_v1;
#define OD_DRUM_PAD_V1_VERSION 1u
#define OD_MAX_DRUM_PADS 16u
typedef struct od_pad_parameters_v1 {
    float gain, pan, tune_semitones; /* [0,4], [-1,1], [-24,24]; tuning captured at note-on */
    uint32_t mute, one_shot, choke_group; /* 0=no choke, 1..16 local to kit */
} od_pad_parameters_v1;
typedef struct od_drum_pad_v1 {
    uint32_t struct_size, version, midi_note;
    od_pad_parameters_v1 parameters;
    const uint8_t* wav_bytes; size_t wav_size; /* NULL + 0 = empty pad; borrowed/copied synchronously */
} od_drum_pad_v1;
#define OD_TRACK_V4_VERSION 4u
typedef struct od_track_v4 {
    uint32_t struct_size, version;
    od_track_v3 base;
    uint32_t instrument_kind, pad_count; /* 0=oscillator, 1=sampler, 2=drum kit */
    const od_drum_pad_v1* pads;
} od_track_v4;
typedef struct od_parameters_v1 { od_track_mix_v1 mix; od_synth_v1 synth; } od_parameters_v1;
typedef struct od_parameters_v2 {
    uint32_t struct_size, version;
    od_parameters_v1 base;
    uint32_t pad_count;
    od_pad_parameters_v1 pads[OD_MAX_DRUM_PADS];
} od_parameters_v2;
typedef struct od_mix_status_v1 { uint64_t generation, submitted, applied; } od_mix_status_v1;
typedef struct od_drum_status_v1 {uint64_t generation;uint32_t pad_count,active_voices;uint64_t triggers[OD_MAX_DRUM_PADS];} od_drum_status_v1;
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
OD_API uint32_t od_track_capacity(void) OD_NOEXCEPT;
OD_API const char* od_error_message(void) OD_NOEXCEPT;
OD_API od_engine* od_create(uint32_t sample_rate, uint32_t max_frames) OD_NOEXCEPT;
OD_API void od_destroy(od_engine*) OD_NOEXCEPT;
OD_API od_result od_load(od_engine*, const od_track*, size_t track_count,
                         const od_note*, size_t note_count, uint64_t length_frames) OD_NOEXCEPT;
/* v2 load sets the initial gate with no audible startup ramp/leak. */
OD_API od_result od_load_v2(od_engine*, const od_track_v2*, size_t track_count,
                            const od_note*, size_t note_count, uint64_t length_frames) OD_NOEXCEPT;
OD_API od_result od_load_v3(od_engine*, const od_track_v3*, size_t track_count,
                            const od_note*, size_t note_count, uint64_t length_frames) OD_NOEXCEPT;
OD_API od_result od_load_v4(od_engine*, const od_track_v4*, size_t track_count,
                            const od_note*, size_t note_count, uint64_t length_frames) OD_NOEXCEPT;
/* Complete mixer/synth/pad targets, same bounded queue and receipt as v1 parameters. */
OD_API od_result od_set_parameters_v2(od_engine*, uint64_t generation, const od_parameters_v2*,
                                     size_t track_count, uint64_t* out_serial) OD_NOEXCEPT;
/* Preview-only priority cancellation, including one-shots already released by the UI.
 * Invalidates older queued live-note commands, leaves transport/score/parameters intact. */
OD_API od_result od_cancel_live(od_engine*) OD_NOEXCEPT;
OD_API od_result od_read_drum_status_v1(const od_engine*, uint64_t generation,uint32_t track,od_drum_status_v1*) OD_NOEXCEPT;
/* Complete mix+synth snapshot. Same bounded queue, generation and status serial as mixer.
 * ADSR latched at note-on; oscillator controls ramp for 10ms on existing voices.
 * Queue-full leaves export state/output serial unchanged. */
OD_API od_result od_set_parameters_v1(od_engine*, uint64_t generation, const od_parameters_v1*,
                                     size_t track_count, uint64_t* out_serial) OD_NOEXCEPT;
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
