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
    od_destroy(e); puts("PASS pure C ABI client"); return 0;
}
