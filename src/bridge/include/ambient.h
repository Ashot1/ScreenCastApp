#ifndef AMBIENT_MEMORY_V3_H
#define AMBIENT_MEMORY_V3_H
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
#define AMBIENT_V3_MAX_TEXT_BYTES 16384u
#define AMBIENT_V3_MAX_PATH_BYTES 16384u
#define AMBIENT_V3_MAX_PAGE_ITEMS 50u
#define AMBIENT_V3_MAX_PAGE_BYTES 262144u
#define AMBIENT_V3_MAX_AFFECTED_TASKS 1024u
#define AMBIENT_V3_OK 0u
#define AMBIENT_V3_INVALID_ARGUMENT 1u
#define AMBIENT_V3_INVALID_UTF8 2u
#define AMBIENT_V3_TOO_LONG 3u
#define AMBIENT_V3_BUFFER_TOO_SMALL 4u
#define AMBIENT_V3_STORAGE_ERROR 5u
#define AMBIENT_V3_UNSUPPORTED_SCHEMA 6u
#define AMBIENT_V3_INVALID_HANDLE 7u
#define AMBIENT_V3_PANIC 8u
#define AMBIENT_V3_RESOURCE_LIMIT 9u
#define AMBIENT_V3_NOT_FOUND 10u
#define AMBIENT_V3_DELETED 11u
#define AMBIENT_V3_STALE_REVISION 12u
#define AMBIENT_V3_TASK_COMPLETED 13u
/* All calls are synchronous; serialize calls on a handle outside the UI actor.
 * Close after accepted calls finish. No workers, callbacks or asynchronous cancellation.
 * Rust owns each store; caller owns all buffers. Handles are nonzero and never reused.
 * All pointers must be live, aligned for their type and readable/writable for their size.
 * Output regions must be mutually disjoint and disjoint from all inputs. No pointers retained.
 * Null array/byte pointers allowed only at zero length/capacity. Text inputs cannot be empty.
 * UTF-8 text is exact, may contain NUL and is not terminated. Source key/version excludes NUL.
 * IDs/revisions are positive <= INT64_MAX. Cursor zero starts a page; page limit is 1..50.
 * Page records and bytes are independent caller-owned arrays. Span offsets index those bytes; bytes_used is the string-buffer length.
 * Pages contain <=50 records and <=256KiB payload including record metadata. A smaller page
 * may be returned to meet the byte cap. Continue with next_after only when has_more=1.
 * Errors preserve outputs except BUFFER_TOO_SMALL writes record/page metadata and no payload.
 * No output ownership crosses the ABI. No hidden serialization/JSON protocol.
 */
typedef struct AmbientText { const uint8_t *bytes; uint32_t length; } AmbientText;
typedef struct AmbientSpan { uint32_t offset; uint32_t length; } AmbientSpan;
typedef struct AmbientTask { uint64_t id; uint64_t revision; AmbientSpan text; uint32_t lifecycle; uint32_t initiative_suppressed; uint32_t plan_completeness; } AmbientTask;
typedef struct AmbientTaskVersion { uint64_t task_id; uint64_t revision; } AmbientTaskVersion;
typedef struct AmbientPage { uint64_t next_after; uint32_t count; uint32_t has_more; uint32_t bytes_used; uint64_t task_revision; uint32_t plan_completeness; } AmbientPage;
/* origin: 0 user (no key/version), 1 external. completeness: 0 unknown, 1 partial, 2 complete.
 * kind: 0 source fact, 1 assumption, 2 user confirmation. Presence flags are exactly 0 or 1.
 * UTC timestamps are signed Unix seconds; unknown external observation time stays absent.
 */
typedef struct AmbientEvidence {
    uint64_t id; uint64_t task_id; int64_t observed_at_utc;
    uint32_t has_observed_at; uint32_t origin; uint32_t completeness; uint32_t kind;
    AmbientSpan text; AmbientSpan namespace; AmbientSpan scope; AmbientSpan object_id;
    uint32_t has_version; AmbientSpan version;
} AmbientEvidence;
typedef struct AmbientSource {
    uint64_t id; uint64_t task_id; uint64_t evidence_id;
    AmbientSpan namespace; AmbientSpan scope; AmbientSpan object_id;
    uint32_t has_version; AmbientSpan version;
} AmbientSource;
typedef struct AmbientLink { uint64_t id; uint64_t other_task_id; int64_t action_at_utc; uint32_t related; uint64_t other_task_revision; AmbientSpan other_task_text; uint32_t other_task_lifecycle; uint32_t other_task_initiative_suppressed; uint32_t other_task_plan_completeness; } AmbientLink;
typedef struct AmbientDecision { uint64_t id; uint64_t evidence_id; int64_t action_at_utc; uint32_t accepted; } AmbientDecision;
typedef struct AmbientSourceKey {
    AmbientText namespace; AmbientText scope; AmbientText object_id;
    uint32_t has_version; AmbientText version;
} AmbientSourceKey;
typedef struct AmbientSourceEvidence {
    AmbientText text; int64_t observed_at_utc; uint32_t has_observed_at;
    uint32_t completeness; uint32_t kind;
} AmbientSourceEvidence;
/* outcome: 0 applied (id >0), 1 forgotten, 2 protected user confirmation (both id=0). */
typedef struct AmbientIngest { uint64_t evidence_id; uint32_t outcome; } AmbientIngest;
uint32_t ambient_v3_open(const uint8_t *path, uint32_t length, uint64_t *output);
uint32_t ambient_v3_close(uint64_t handle);
uint32_t ambient_v3_manual_pause(uint64_t handle, uint32_t *output);
uint32_t ambient_v3_set_manual_pause(uint64_t handle, uint32_t paused);
uint32_t ambient_v3_create_task(uint64_t handle, AmbientText text, uint64_t *output);
uint32_t ambient_v3_edit_task(uint64_t handle, uint64_t id, uint64_t revision, AmbientText text);
uint32_t ambient_v3_delete_task(uint64_t handle, uint64_t id, uint64_t revision, const AmbientTaskVersion *others, uint32_t count);
uint32_t ambient_v3_task(uint64_t handle, uint64_t id, AmbientTask *output, uint8_t *bytes, uint32_t capacity);
uint32_t ambient_v3_tasks(uint64_t handle, uint64_t after, uint32_t limit, AmbientPage *page, AmbientTask *records, uint32_t record_capacity, uint8_t *bytes, uint32_t byte_capacity);
uint32_t ambient_v3_evidence(uint64_t handle, uint64_t task, uint64_t after, uint32_t limit, AmbientPage *page, AmbientEvidence *records, uint32_t record_capacity, uint8_t *bytes, uint32_t byte_capacity);
uint32_t ambient_v3_sources(uint64_t handle, uint64_t task, uint64_t after, uint32_t limit, AmbientPage *page, AmbientSource *records, uint32_t record_capacity, uint8_t *bytes, uint32_t byte_capacity);
uint32_t ambient_v3_links(uint64_t handle, uint64_t task, uint64_t after, uint32_t limit, AmbientPage *page, AmbientLink *records, uint32_t record_capacity, uint8_t *bytes, uint32_t byte_capacity);
uint32_t ambient_v3_decisions(uint64_t handle, uint64_t task, uint64_t after, uint32_t limit, AmbientPage *page, AmbientDecision *records, uint32_t record_capacity, uint8_t *bytes, uint32_t byte_capacity);
/* evidence=0 previews task deletion. Otherwise preview deletion of the selected evidence.
 * Before delete, supply exactly the additional affected tasks and their previewed revisions.
 * Core recomputes the set and fences all revisions atomically. At most 1023 other live tasks.
 */
uint32_t ambient_v3_deletion_impact(uint64_t handle, uint64_t task, uint64_t evidence, uint64_t after, uint32_t limit, AmbientPage *page, AmbientTask *records, uint32_t record_capacity, uint8_t *bytes, uint32_t byte_capacity);
uint32_t ambient_v3_add_note(uint64_t handle, uint64_t task, uint64_t revision, AmbientText text, int64_t action_at_utc, uint64_t *output);
uint32_t ambient_v3_edit_evidence(uint64_t handle, uint64_t id, uint64_t task, uint64_t revision, AmbientText text, int64_t action_at_utc);
uint32_t ambient_v3_delete_evidence(uint64_t handle, uint64_t id, uint64_t task, uint64_t revision, const AmbientTaskVersion *others, uint32_t count);
uint32_t ambient_v3_move_evidence(uint64_t handle, uint64_t id, uint64_t from, uint64_t from_revision, uint64_t to, uint64_t to_revision, int64_t action_at_utc);
uint32_t ambient_v3_set_link(uint64_t handle, uint64_t a, uint64_t a_revision, uint64_t b, uint64_t b_revision, uint32_t related, int64_t action_at_utc);
/* Historical ingestion cannot create tasks, supply user confirmation, choose a new evidence ID,
 * or bypass a forgotten exact key with another version. One record per (task, exact key).
 * Explicit bind is a separate user action; it never revives a deleted identity or old content.
 */
uint32_t ambient_v3_ingest_source(uint64_t handle, uint64_t task, uint64_t revision, const AmbientSourceKey *key, const AmbientSourceEvidence *evidence, AmbientIngest *output);
uint32_t ambient_v3_bind_source(uint64_t handle, uint64_t task, uint64_t revision, const AmbientSourceKey *key, const AmbientSourceEvidence *evidence, int64_t action_at_utc, uint64_t *output);
/* v3: lifecycle 0 open/1 completed; suppression 0/1. Page task_revision=0 is a global page.
 * Plan completeness 0 unknown/1 partial/2 complete. ready is computed, never persisted.
 * Completion references are exact strings, 1..2048 UTF-8 bytes without NUL, max 32.
 * Selected keep IDs must already have accepted links. Impact excludes those retained links.
 * Every identity remains terminal after deletion; mutations fence the owning task revision.
 */
typedef struct AmbientStepInput { AmbientText description; uint32_t has_prompt; AmbientText prompt; uint32_t has_reference; AmbientText reference; } AmbientStepInput;
typedef struct AmbientStep { uint64_t id; uint64_t task_id; AmbientSpan description; uint32_t has_prompt; AmbientSpan prompt; uint32_t has_reference; AmbientSpan reference; uint32_t has_completed; int64_t completed_at_utc; uint32_t ready; } AmbientStep;
typedef struct AmbientCheck { uint64_t id; uint64_t task_id; uint64_t step_id; AmbientSpan description; uint32_t result; uint32_t has_confirmation; int64_t confirmed_at_utc; } AmbientCheck;
typedef struct AmbientDependency { uint64_t id; uint64_t task_id; uint64_t predecessor; uint64_t dependent; } AmbientDependency;
typedef struct AmbientReference { uint64_t id; uint64_t task_id; AmbientSpan text; } AmbientReference;
uint32_t ambient_v3_add_step(uint64_t handle, uint64_t task, uint64_t revision, const AmbientStepInput *input, uint64_t *output);
uint32_t ambient_v3_edit_step(uint64_t handle, uint64_t task, uint64_t revision, uint64_t id, const AmbientStepInput *input);
uint32_t ambient_v3_delete_step(uint64_t handle, uint64_t task, uint64_t revision, uint64_t id);
uint32_t ambient_v3_set_plan_completeness(uint64_t handle, uint64_t task, uint64_t revision, uint32_t value);
uint32_t ambient_v3_set_step_completed(uint64_t handle, uint64_t task, uint64_t revision, uint64_t id, uint32_t has_completed, int64_t at);
uint32_t ambient_v3_add_check(uint64_t handle, uint64_t task, uint64_t revision, uint64_t step, AmbientText description, uint64_t *output);
uint32_t ambient_v3_edit_check(uint64_t handle, uint64_t task, uint64_t revision, uint64_t id, AmbientText description);
uint32_t ambient_v3_delete_check(uint64_t handle, uint64_t task, uint64_t revision, uint64_t id);
uint32_t ambient_v3_set_check_result(uint64_t handle, uint64_t task, uint64_t revision, uint64_t id, uint32_t result, uint32_t has_confirmation, int64_t at);
uint32_t ambient_v3_add_dependency(uint64_t handle, uint64_t task, uint64_t revision, uint64_t predecessor, uint64_t dependent, uint64_t *output);
uint32_t ambient_v3_delete_dependency(uint64_t handle, uint64_t task, uint64_t revision, uint64_t id);
uint32_t ambient_v3_set_initiative_suppressed(uint64_t handle, uint64_t task, uint64_t revision, uint32_t value);
uint32_t ambient_v3_reopen_task(uint64_t handle, uint64_t task, uint64_t revision);
uint32_t ambient_v3_complete_task(uint64_t handle, uint64_t task, uint64_t revision, AmbientText summary, const uint64_t *keep, uint32_t keep_count, const AmbientText *references, uint32_t reference_count, const AmbientTaskVersion *others, uint32_t other_count);
uint32_t ambient_v3_completion_impact(uint64_t handle, uint64_t task, const uint64_t *keep, uint32_t keep_count, uint64_t after, uint32_t limit, AmbientPage *page, AmbientTask *records, uint32_t record_capacity, uint8_t *bytes, uint32_t byte_capacity);
uint32_t ambient_v3_steps(uint64_t handle, uint64_t task, uint64_t after, uint32_t limit, AmbientPage *page, AmbientStep *records, uint32_t record_capacity, uint8_t *bytes, uint32_t byte_capacity);
uint32_t ambient_v3_dependencies(uint64_t handle, uint64_t task, uint64_t after, uint32_t limit, AmbientPage *page, AmbientDependency *records, uint32_t record_capacity, uint8_t *bytes, uint32_t byte_capacity);
uint32_t ambient_v3_references(uint64_t handle, uint64_t task, uint64_t after, uint32_t limit, AmbientPage *page, AmbientReference *records, uint32_t record_capacity, uint8_t *bytes, uint32_t byte_capacity);
uint32_t ambient_v3_checks(uint64_t handle, uint64_t task, uint64_t step, uint64_t after, uint32_t limit, AmbientPage *page, AmbientCheck *records, uint32_t record_capacity, uint8_t *bytes, uint32_t byte_capacity);
#ifdef __cplusplus
}
#endif
#endif

#include "ambient_auth.h"
