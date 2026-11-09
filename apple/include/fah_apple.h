#ifndef FAH_APPLE_H
#define FAH_APPLE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
// All tensors are contiguous float32. Callback outputs are caller-owned.
// Return zero on success; otherwise write a NUL-terminated error (capacity 1024).
// detect: RGB CHW [3,640,640] -> [6,8400] (cx,cy,w,h,left,right).
// pose: RGB CHW [3,320,320], ray [sx,ox,sy,oy], condition [3].
// pose output: uv[922], xyz[1383], conf[461], vis[461], depth[1], verify[2].
typedef struct {
    uint32_t abi_version;
    void *context;
    int32_t (*detect)(void *, const float *, float *, char *);
    int32_t (*pose)(void *, const float *, const float *, const float *, float *, char *);
    void (*destroy)(void *);
} FahMetalCallbacks;
typedef struct FahSession FahSession;
// Takes ownership of a valid v1 callback context, even when initialization fails.
// Callbacks must be callable on any host thread, serially. Never reenter this API.
// Invalid callback tables are rejected without taking ownership.
FahSession *fah_apple_create(const char *models, FahMetalCallbacks callbacks, char **error);
// Copies RGBA8 pixels. Calls on a session are serialized; destroy must not overlap.
// On success returns owned JSON; on failure returns NULL and owned error text.
char *fah_apple_process(FahSession *, const uint8_t *, size_t bytes,
                        uint32_t width, uint32_t height, size_t stride, int32_t reset, char **error);
void fah_apple_destroy(FahSession *);
void fah_apple_string_free(char *);
#ifdef __cplusplus
}
#endif
#endif
