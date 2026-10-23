#include "open_daw/api.h"
#include <stdio.h>
#include <stdlib.h>
int main(void) {
    od_engine* e = od_create(48000, 128);
    const od_track track = {0.5f, 0.0f, NULL, 60, 0};
    const od_note note = {1, 0, 900, 0, 69, 100};
    float audio[256] = {0};
    od_telemetry t = {0};
    if (!e || od_load(e, &track, 1, &note, 1, 2000) || od_play(e) || od_render(e, audio, 128) || od_read_telemetry(e, &t)) {
        fprintf(stderr, "%s\n", od_error_message()); od_destroy(e); return 1;
    }
    if (t.position != 128 || audio[100] == 0) { od_destroy(e); return 2; }
    {
        od_track_v3 v3 = {sizeof(od_track_v3), OD_TRACK_V3_VERSION,
            {sizeof(od_track_v2), OD_TRACK_V2_VERSION, {0.5f, 0.0f, NULL, 60, 0}, 0, 0},
            {sizeof(od_synth_v1), OD_SYNTH_V1_VERSION, 2, 1.0f, 0.25f, 0.006f, 0.06f, 0.75f, 0.06f}};
        od_parameters_v1 parameters = {{0.5f, 0.0f, 0, 0}, v3.synth};
        od_mix_status_v1 status = {0};
        uint64_t serial = 0;
        if (od_load_v3(e, &v3, 1, &note, 1, 2000) || od_read_mix_status_v1(e, &status) ||
            od_set_parameters_v1(e, status.generation, &parameters, 1, &serial) || serial != 1) { od_destroy(e); return 3; }
    }
    od_destroy(e); puts("PASS pure C ABI client"); return 0;
}
