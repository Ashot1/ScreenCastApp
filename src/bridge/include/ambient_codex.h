#ifndef AMBIENT_CODEX_V1_H
#define AMBIENT_CODEX_V1_H
#include "ambient.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Separate Codex ABI1 on the existing memory handle/Store. Existing ABIs unchanged.
 * Synchronous off UI; no callbacks, retained pointers, filesystem I/O or JSON output.
 * Caller buffers are live, aligned, disjoint until return; errors preserve outputs.
 * Status: source1 codes plus16 unsupported format/17 binding conflict.
 * Settings<=64,4096 UTF8 path bytes,16KiB row/1MiB total; ID/revision1..INT64_MAX.
 * save id0/expected0 creates; editing preserves id and issues a fresh shared revision.
 * begin gates task/settings/epoch/High/session/report/pause. cursor0 reads recent;
 * other cursor values MUST be opaque Core tokens from the current window.
 * Native permits before EVERY bounded I/O, nofollow all path components, regular
 * files only, actual path/dev/inode verified before/after. Header read<=262145,
 * complete session_meta<=262144. plan returns nominal[start,end); native reads
 * [start-(start>0),end),<=1048577 bytes including the preceding delimiter probe.
 * No append beyond observed size. Recheck header/path/dev/inode/no truncation.
 * finish/abort ONLY after actual syscall return and closure of ALL operation FDs.
 * cancel never releases busy; release during I/O returns4. One job/window, no queue.
 * Window<=4096 physical lines/128 events/262144 normalized bytes. Output capacities
 * >=128 records/262144 bytes. Preview>=16384; empty/oversized text cannot bind.
 * bind only Core-owned window/index, rechecks all gates, atomic evidence+metadata+
 * ordinal/hash/task revision. Metadata pages<=50/256KiB including all returned data.
 * EVERY saved binding has fixed coverage local_records_only/local_window. Detailed
 * window receipts are ephemeral. Kinds1 user(human_unverified),2 agent,3 plan,
 * 4 delegated create,5 delegated send,6 observed start,7 completed,8 failed,9 aborted.
 * Neither plan nor completion is user confirmation or proof of task completion.
 * Reasons bits0 fragment/1 limit/2 malformed/3 unsupported/4 ordinal/5 compacted/
 * 6 inherited subagent prefix filtered/7 missing or conflicting turn identity.
 */
typedef struct { uint64_t id, revision; AmbientSpan path; uint32_t bytes_used, reserved; } AmbientCodexSettings;
typedef struct { uint64_t device, inode, size; } AmbientCodexFile;
typedef struct { uint64_t start, end; } AmbientCodexPlan;
typedef struct { uint64_t ordinal; int64_t observed_at_utc; uint32_t kind, completeness, has_time, reserved; AmbientSpan text, turn_id, item_id, object_id, version; } AmbientCodexEvent;
typedef struct { uint64_t cursor, next_before; int64_t observed_at_utc; uint32_t reasons, physical_lines, count, bytes_used; AmbientSpan thread_id; } AmbientCodexWindow;
typedef struct { uint64_t binding_id, evidence_id, selection_id, settings_revision, accepted_ordinal; uint32_t kind, reserved; AmbientSpan turn_id, item_id; } AmbientCodexMetadata;
uint32_t ambient_codex_v1_settings_get(uint64_t,uint64_t,AmbientCodexSettings *,uint8_t *,uint32_t);
uint32_t ambient_codex_v1_settings_at(uint64_t,uint32_t,AmbientCodexSettings *,uint8_t *,uint32_t);
uint32_t ambient_codex_v1_settings_save(uint64_t,uint64_t,uint64_t,AmbientText,uint64_t *,uint64_t *);
uint32_t ambient_codex_v1_settings_delete(uint64_t,uint64_t,uint64_t);
uint32_t ambient_codex_v1_begin(uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t,uint64_t *);
uint32_t ambient_codex_v1_permit(uint64_t,uint64_t);
uint32_t ambient_codex_v1_plan(uint64_t,uint64_t,AmbientCodexFile,const uint8_t *,uint32_t,AmbientCodexPlan *);
uint32_t ambient_codex_v1_finish(uint64_t,uint64_t,const uint8_t *,uint32_t,int64_t);
uint32_t ambient_codex_v1_cancel(uint64_t,uint64_t);
uint32_t ambient_codex_v1_abort(uint64_t,uint64_t);
uint32_t ambient_codex_v1_release(uint64_t,uint64_t);
uint32_t ambient_codex_v1_window(uint64_t,uint64_t,AmbientCodexWindow *,AmbientCodexEvent *,uint32_t,uint8_t *,uint32_t);
uint32_t ambient_codex_v1_preview(uint64_t,uint64_t,uint32_t,uint8_t *,uint32_t,uint32_t *);
uint32_t ambient_codex_v1_bind(uint64_t,uint64_t,uint32_t,int64_t,uint64_t *);
uint32_t ambient_codex_v1_saved(uint64_t,uint64_t,uint64_t,AmbientPage *,AmbientCodexMetadata *,uint32_t,uint8_t *,uint32_t);
#ifdef __cplusplus
}
#endif
#endif
