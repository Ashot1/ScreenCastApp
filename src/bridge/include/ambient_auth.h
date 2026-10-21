#ifndef AMBIENT_AUTH_V1_H
#define AMBIENT_AUTH_V1_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
/* Independent auth ABI1. Calls use a dedicated serial executor, never memory's lock.
 * Record and byte buffers belong to the caller and must not overlap any input.
 * Capacities >=65536 are required BEFORE prepare/callback/dispatch: a small
 * buffer never consumes a POST. All strings are UTF-8 spans, no terminator.
 * Unused spans/flags must be zero. Registration: 0 none,1 dynamic pending,
 * 2 pending issued,3 bound. scope_count<=64. Outputs unchanged on errors.
 * cancel is a short thread-safe request; close returns BUSY while a call runs.
 * Native must await accepted I/O and connection cleanup before close/termination.
 * Status:0 OK,1 input,2 sequence,3 cancelled,4 expired,5 transport unknown,
 * 6 response,7 identity,8 denied,9 limit,10 internal,11 buffer,12 handle,13 busy.
 * No SDK errors or secrets are returned in diagnostic text. No handle reuse.
 */
typedef struct { uint32_t offset; uint32_t length; } AmbientAuthSpan;
typedef struct {
    uint32_t registration;
    uint32_t has_hint;
    uint32_t has_credentials;
    uint32_t scope_count;
    int64_t expires_at;
    uint32_t bytes_used;
    uint32_t reserved;
    AmbientAuthSpan host, client_id, issuer, subject, hint, nonce, access, refresh, url;
    AmbientAuthSpan scopes[64];
} AmbientAuthRecord;
uint32_t ambient_auth_v1_begin(uint32_t refresh, const AmbientAuthRecord *record, const uint8_t *bytes, uint32_t length, const uint8_t *redirect, uint32_t redirect_length, uint64_t *handle);
uint32_t ambient_auth_v1_prepare(uint64_t handle, AmbientAuthRecord *record, uint8_t *bytes, uint32_t capacity);
uint32_t ambient_auth_v1_callback(uint64_t handle, const uint8_t *url, uint32_t length, AmbientAuthRecord *record, uint8_t *bytes, uint32_t capacity);
uint32_t ambient_auth_v1_dispatch(uint64_t handle, const uint8_t *client_id, uint32_t length, AmbientAuthRecord *record, uint8_t *bytes, uint32_t capacity);
uint32_t ambient_auth_v1_cancel(uint64_t handle);
uint32_t ambient_auth_v1_close(uint64_t handle);
#ifdef __cplusplus
}
#endif
#endif
