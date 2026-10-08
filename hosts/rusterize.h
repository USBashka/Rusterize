#ifndef RUSTERIZE_H
#define RUSTERIZE_H
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
/* All calls for one handle must be on its creating thread. Data/error pointers
 * remain valid until the next mutating call on that handle. Never free them. */
uint64_t rusterize_create(void);
void rusterize_destroy(uint64_t id);
uint32_t rusterize_status(uint64_t id);
const char *rusterize_title(uint64_t id);
/* 0..3: f32 bits of width,height,min_width,min_height. 4..6: RGB titlebar,
 * text,border colour (UINT32_MAX=OS default). 7: resizable. 8: revision. */
uint32_t rusterize_window_option(uint64_t id, uint32_t option);
uint32_t rusterize_event(uint64_t id, uint32_t kind, float x, float y, float dx, float dy, uint32_t detail, uint32_t flags);
uint32_t rusterize_frame(uint64_t id, float width, float height, float scale, double seconds);
const uint8_t *rusterize_data(uint64_t id);
size_t rusterize_len(uint64_t id);
const char *rusterize_error(uint64_t id);
#define RUSTERIZE_REDRAW 1u
#define RUSTERIZE_ANIMATE 2u
#define RUSTERIZE_EXIT 4u
#define RUSTERIZE_FAILED 0x80000000u
#ifdef __cplusplus
}
#endif
#endif
