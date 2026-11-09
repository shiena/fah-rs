#ifndef FAH_ENGINE_H
#define FAH_ENGINE_H
#include <stddef.h>
#include <stdint.h>
#if defined(_WIN32)
#define FAH_CALL __cdecl
#define FAH_API __declspec(dllimport)
#else
#define FAH_CALL
#define FAH_API
#endif
#ifdef __cplusplus
extern "C" {
#endif
#define FAH_ENGINE_ABI 1u
#define FAH_OK 0
#define FAH_ERROR 1
#define FAH_BUFFER_TOO_SMALL 2
#define FAH_AUTO 0u
#define FAH_CPU 1u
#define FAH_VULKAN 2u
#define FAH_CUDA 3u
#define FAH_DIRECTML 4u
#define FAH_METAL 5u
/* Handles are never pointers. Zero is invalid. All structures use native alignment. */
typedef uint64_t FahHandle;
typedef struct FahConfig {
    uint32_t abi_version, struct_size;
    const char *models_utf8;
    /* Required absolute path to libFAHAppleNative.dylib on macOS; NULL elsewhere. */
    const char *native_library_utf8;
    uint32_t backend;
} FahConfig;
/* Top-left origin, RGBA8, positive row stride; no automatic rotation or mirroring.
   Pixels must remain readable until process returns; the library copies them.
   reset=1 starts a new tracking sequence (use after seek/source changes). */
typedef struct FahImage {
    const uint8_t *data;
    size_t byte_len;
    uint32_t width, height;
    size_t stride;
    uint32_t reset;
} FahImage;
typedef struct FahHand {
    uint32_t right;
    float score, presence, side;
    float joints2d[21][2];
    float joints3d[21][3];
} FahHand;
typedef struct FahResult {
    uint32_t abi_version, struct_size, width, height, backend, hand_count;
    uint64_t frame;
    double detector_ms, pose_ms, total_ms;
    FahHand hands[2];
} FahResult;
/* All functions are synchronous. Use a host worker thread for inference.
   Calls on one session serialize. Different sessions may run concurrently.
   Destroy prevents new calls; calls already in progress may finish.
   Join host tasks before unloading the engine library.
   All non-NULL pointers must be aligned, valid, and accessible for the stated size.
   Input/output buffers must not overlap. Strings are NUL-terminated UTF-8. */
/* After all tasks/handles are released, call shutdown before process exit or
   unloading the plugin. This releases process-wide Vulkan state on Windows and Linux.
   No other users of the shared fah_backend library may still be active. */
FAH_API int32_t FAH_CALL fah_engine_shutdown(void);
FAH_API uint32_t FAH_CALL fah_engine_abi_version(void);
FAH_API int32_t FAH_CALL fah_engine_create(const FahConfig *, FahHandle *session);
FAH_API int32_t FAH_CALL fah_engine_destroy(FahHandle session);
/* On failure, *result=0. Results remain valid after session destruction. */
FAH_API int32_t FAH_CALL fah_engine_process(FahHandle session, const FahImage *, FahHandle *result);
FAH_API int32_t FAH_CALL fah_engine_result_data(FahHandle result, FahResult *, size_t struct_size);
/* required includes the NUL terminator. Query with NULL,0, then allocate and retry.
   A small buffer is not modified. No second inference is needed to retry. */
FAH_API int32_t FAH_CALL fah_engine_result_json(FahHandle result, char *buffer, size_t capacity, size_t *required);
FAH_API int32_t FAH_CALL fah_engine_result_destroy(FahHandle result);
/* Thread-local error; copy immediately after FAH_ERROR on the same thread.
   Returns required bytes including NUL; truncates safely when capacity is short.
   Success clears the previous error. This function itself does not clear it. */
FAH_API size_t FAH_CALL fah_engine_last_error(char *buffer, size_t capacity);
#ifdef __cplusplus
}
#endif
#endif
