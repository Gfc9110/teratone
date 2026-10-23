#include "open_daw/api.h"
#include <stdio.h>
#include <stdlib.h>
int main(void) {
    if(od_track_capacity()!=OD_MAX_TRACKS)return 5;
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
    {
        const od_synth_v1 synth = {sizeof(od_synth_v1),1,0,1,0.5f,0.001f,0,1,0.01f};
        od_drum_pad_v1 pad = {sizeof(od_drum_pad_v1), OD_DRUM_PAD_V1_VERSION, 35, {1,0,0,0,1,0}, NULL,0};
        od_track_v4 v4 = {0}; od_parameters_v2 parameters = {0}; od_mix_status_v1 mix = {0};
        od_drum_status_v1 drums = {0}; uint64_t serial = 0;
        v4.struct_size=sizeof(v4);v4.version=OD_TRACK_V4_VERSION;v4.instrument_kind=2;v4.pad_count=1;v4.pads=&pad;
        v4.base.struct_size=sizeof(od_track_v3);v4.base.version=OD_TRACK_V3_VERSION;
        v4.base.base.struct_size=sizeof(od_track_v2);v4.base.base.version=OD_TRACK_V2_VERSION;v4.base.base.base.gain=0.5f;
        v4.base.synth=synth;
        parameters.struct_size=sizeof(parameters);parameters.version=2;parameters.base.mix.gain=0.5f;
        parameters.base.synth=v4.base.synth;parameters.pad_count=1;parameters.pads[0]=pad.parameters;
        if(od_load_v4(e,&v4,1,NULL,0,2000) || od_read_mix_status_v1(e,&mix) ||
           od_set_parameters_v2(e,mix.generation,&parameters,1,&serial) || serial!=1 || od_cancel_live(e) ||
           od_read_drum_status_v1(e,mix.generation,0,&drums) || drums.pad_count!=1) {od_destroy(e);return 4;}
    }
    od_destroy(e); puts("PASS pure C ABI client"); return 0;
}
