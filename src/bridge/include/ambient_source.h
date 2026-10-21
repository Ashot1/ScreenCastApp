#ifndef AMBIENT_SOURCE_V1_H
#define AMBIENT_SOURCE_V1_H
#include "ambient.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Local-source ABI1. Memory3/auth1/model2 layouts unchanged. Synchronous off UI,
 * same Store, no callbacks/retained pointers/cross-language allocation/JSON input.
 * Source bytes are untrusted, never filesystem paths opened by Rust.
 * Status 0 OK/1 invalid/2 paused/3 cancelled/4 busy/5 storage/6 limit/7 handle/
 * 8 panic/9 report/10 High capability/11 stale/12 session/13 completed/14 missing/15 deleted.
 * Aligned live disjoint caller buffers until return; errors preserve outputs.
 * settings save: actual absolute UTF-8 root<=4096, <=16 relative prefixes<=1024,
 * total<=16KiB, <=64 roots/1MiB. expected=0 only for absent root. Durable global
 * revision never reused after delete/recreate. get/at capacity16 spans/16384 bytes.
 * begin: explicit current task/revision+settings revision, current session/report,
 * High capability/epoch/pause checked by Core. No persisted read permission.
 * permit before every bounded native I/O portion, directory flag0/1, path relative
 * allowlist only (empty is root metadata). allowed0 is an exclusion, not an error.
 * Cancellation sets the shared Core atomic without waiting for Store/SQLite; it
 * invalidates next permit/result immediately, never proves syscall return.
 * One active reader; cancel does not release busy. finish/abort MUST be called only
 * after actual I/O return and closure of ALL operation FDs. Native owns this boundary.
 * finish retains one Core observation until bind/release/new begin; no more reads.
 * abort invalidates failed/cancelled observation after cleanup. release returns busy
 * while native I/O is active. Memory close cancels; native drains before closing it.
 * finish: <=256 files,1MiB actual bytes (includes overflow probe),256 dirs,4096 shared
 * directory+packed entries; each file before allocation/read: config64KiB,HEAD/loose
 * 4096,packed512KiB,relative path2048,depth16,256 branches. Native enforces pre-read
 * limits and nofollow/identity checks; Core enforces ingress/parser limits too.
 * reasons bit0 excluded/1 limit/2 unavailable/3 invalid-name/4 malformed/5 unsupported/
 * 6 changed/7 symbolic. Changed/unsupported never supply branch evidence.
 * observation output: capacity256 branches/1048576 bytes (complete bounded result).
 * branch preview: exact Core text,capacity16384; bind accepts only observation index.
 * SourceKey location+full-ref and version OID are independent of read grant.
 * Settings/task/epoch/pause/cancel checked before preview/bind and model use/copy.
 * Root metadata checks are application-level, not atomic filesystem sandboxing.
 */
typedef struct { uint64_t revision; AmbientSpan path; uint32_t exclusion_count, bytes_used; } AmbientSourceSettings; /*24*/
typedef struct { AmbientText path; const uint8_t *bytes; uint32_t length; } AmbientSourceFile;
typedef struct { uint32_t bytes, files, directories, entries; } AmbientSourceStats; /*16*/
typedef struct { AmbientSpan full_ref, oid; uint32_t has_oid, head; } AmbientSourceBranch; /*24;head0unknown/1current/2other/3detached/4unborn*/
typedef struct { int64_t started_at_utc, ended_at_utc; uint32_t completeness, reasons, branch_count, bytes_used; } AmbientSourceObservation; /*32*/
uint32_t ambient_source_v1_settings_get(uint64_t, AmbientText, AmbientSourceSettings *, AmbientSpan *, uint32_t, uint8_t *, uint32_t);
uint32_t ambient_source_v1_settings_at(uint64_t, uint32_t, AmbientSourceSettings *, AmbientSpan *, uint32_t, uint8_t *, uint32_t);
uint32_t ambient_source_v1_settings_save(uint64_t, AmbientText, uint64_t, const AmbientText *, uint32_t, uint64_t *);
uint32_t ambient_source_v1_settings_delete(uint64_t, AmbientText, uint64_t);
uint32_t ambient_source_v1_begin(uint64_t, uint64_t, uint64_t, AmbientText, uint64_t, uint64_t *);
uint32_t ambient_source_v1_permit(uint64_t, uint64_t, AmbientText, uint32_t, uint32_t *);
uint32_t ambient_source_v1_cancel(uint64_t, uint64_t);
uint32_t ambient_source_v1_finish(uint64_t, uint64_t, const AmbientSourceFile *, uint32_t, AmbientSourceStats, int64_t, int64_t, uint32_t);
uint32_t ambient_source_v1_abort(uint64_t, uint64_t);
uint32_t ambient_source_v1_release(uint64_t, uint64_t);
uint32_t ambient_source_v1_observation(uint64_t, uint64_t, AmbientSourceObservation *, AmbientSourceBranch *, uint32_t, uint8_t *, uint32_t);
uint32_t ambient_source_v1_preview(uint64_t, uint64_t, uint32_t, uint8_t *, uint32_t, uint32_t *);
uint32_t ambient_source_v1_bind(uint64_t, uint64_t, uint32_t, int64_t, uint64_t *);
#ifdef __cplusplus
}
#endif
#endif
