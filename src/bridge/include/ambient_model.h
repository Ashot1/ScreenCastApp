#ifndef AMBIENT_MODEL_V6_H
#define AMBIENT_MODEL_V6_H
#include <stdint.h>
#include "ambient.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Model ABI6 replaces ABI5 without compatibility (dispatch gains the root folder of local
 * plugin servers; every other export is renamed unchanged). memory3/auth1/source1/Codex1/
 * integration1 stay; plugin ABI2 (ambient_plugin.h) replaces plugin ABI1.
 * All calls are synchronous off UI; caller owns aligned/live/disjoint buffers until
 * return. No callbacks, retained pointers, JSON input or cross-language allocations.
 * Status:0 OK/1 invalid/2 paused/3 cancelled/4 busy/5 storage/6 limit/7 handle/
 * 8 panic/9 report/10 High capability/11 stale/12 session/13 completed.
 * IDs/revisions are positive INT64, epoch is nonzero process-only. Token is 1..16384
 * printable ASCII bytes and is never stored. Strings are exact UTF-8, NUL allowed.
 * Errors leave every output unchanged. Capacities are checked BEFORE mutation.
 * One exact preview -> one begin -> one dispatch. Without an enabled plugin with approved
 * tools that dispatch makes exactly one possible-send commit and at most one POST, as before.
 * With plugin tools it runs the whole tool loop: Core admits every POST (a possible-send
 * commit before each, at most 5) and every call (at most 8), and the last POST has no tools;
 * the loop ends when Core refuses the next step. No retry/redirect/fallback. Busy lasts
 * through actual HTTP/runtime/accounting cleanup of every POST and call, and through the stop
 * of every local server process the dispatch started (stdin closed, SIGTERM, SIGKILL to the
 * group; a stop not confirmed within its 2 s bounds is recorded in the call log and busy is
 * released anyway). root: absolute UTF-8 path, 1..1024 bytes, no NUL (1 otherwise, before
 * any Core use); a local server of plugin P runs in the private folder <root>/<P>, created
 * 0700 if missing. Native deletes that folder after the plugin is removed.
 * Plugin keys: <=64 records {plugin, server [a-z0-9-]{1,32}, generation, key ASCII 33..126
 * 1..16384 bytes}, borrowed for the call like the token and never stored; Core sees only
 * (plugin, server, generation). The key of a remote server is its bearer token; of a local
 * server, the compact JSON object of its variable values (passed only in its environment). Duplicates or invalid records: 1 before any Core use.
 * cancel requests stop, close returns busy until actual return; every begin needs close.
 * Memory close cancels but a model lease retains Store for late usage accounting.
 * Cancellation/timeout does not prove that remote generation stopped.
 * session/available concern current High Fast permission; nothing is persisted.
 * feed/details are read-only on the existing Store and never acquire model transport
 * or High. Their JSON OUTPUT is the complete Core projection, with all framing and
 * metadata counted: feed128 rows/4MiB, each serialized row32KiB, details256KiB.
 * No partial lists or duplicated citation texts in feed. Details show stale proposal
 * with an empty citation expansion when grounds changed. Reading grants no action.
 * copy/show/Continue revalidate current session/report/High/pause and all grounds.
 * copy returns exact current message<=16KiB; only native performs explicit clipboard.
 * visibility changes only visibility; hide works locally without High. Clear discussion
 * checks exact task/workspace versions and erases only derived help, even without High.
 * New context (mode2) confirms full replacement/show on SUCCESS; it includes no old
 * proposals/replies. Continue(mode1) has zero notes and includes the entire existing
 * set including closed cards. Optional reply has exact proposal/content revision;
 * no reply uses id/revision/length0. Prepared context/preview never writes SQLite/ledger.
 * Publication follows accounting in one Immediate transaction and checks all fences,
 * all exact serialized limits and the existing shared64MiB bound; failure keeps the
 * previous discussion and the actual expense. No receipt/capability/HTTP body is saved.
 * prepare capacity>=262144, feed>=4194304, details>=262144, copy>=16384.
 * Span is a UTF-8 byte offset/length. No result getter or public process receipt owner.
 */
typedef struct { uint32_t offset,length; } AmbientModelSpan;
typedef struct {
 uint64_t id; int64_t at_utc;
 uint32_t stage,outcome,expected_model,expected_effort,expected_tier;
 uint32_t actual_model,actual_effort,actual_tier,has_usage,purpose;
 uint64_t input_tokens,output_tokens,total_tokens;
} AmbientModelAttempt; /*80 bytes/alignment8; tags unchanged*/
typedef struct {
 uint64_t attempts,input_tokens,output_tokens,total_tokens,unknown;
 AmbientModelAttempt latest; uint32_t has_latest,reserved;
} AmbientModelSummary; /*128 bytes*/
typedef struct {
 AmbientModelAttempt attempt; uint64_t published_task,workspace_revision;
} AmbientModelCompletion; /*96 bytes; both publication fields0 when absent*/
typedef struct {
 uint64_t request_id,task_id,revision,workspace_revision;
 AmbientModelSpan context; uint32_t mode,reserved;
} AmbientAssistancePreview; /*48 bytes; mode1 Continue/2 NewContext*/
typedef struct {
 uint64_t task_id,revision,workspace_revision,proposal_id,content_revision;
 uint32_t mode,reserved;
} AmbientAssistanceRequest; /*48 bytes; reserved0*/
typedef struct { uint64_t proposal_id,content_revision; } AmbientProposalTarget;
typedef struct {
 uint64_t plugin; AmbientText server; uint64_t generation; AmbientText key;
} AmbientPluginKey; /*48 bytes/alignment8*/
uint32_t ambient_model_v6_session(uint64_t memory,uint64_t epoch,uint32_t valid,uint32_t reported);
uint32_t ambient_model_v6_available(uint64_t memory,uint32_t *out);
uint32_t ambient_model_v6_workspace_revision(uint64_t memory,uint64_t task,uint64_t *out);
uint32_t ambient_model_v6_prepare(uint64_t memory,AmbientAssistanceRequest request,const uint64_t *notes,uint32_t count,const uint8_t *reply,uint32_t reply_length,AmbientAssistancePreview *out,uint8_t *bytes,uint32_t capacity);
uint32_t ambient_model_v6_clear(uint64_t memory);
/* Discard an unused exact preview. A late old discard cannot clear a newer preview. */
uint32_t ambient_model_v6_discard_preview(uint64_t memory,uint64_t request);
uint32_t ambient_model_v6_begin(uint64_t memory,uint32_t reported,uint32_t profile,int64_t at_utc,uint64_t request,uint64_t *out);
uint32_t ambient_model_v6_dispatch(uint64_t operation,const uint8_t *token,uint32_t token_length,const AmbientPluginKey *keys,uint32_t key_count,AmbientText root,AmbientModelCompletion *out);
uint32_t ambient_model_v6_cancel(uint64_t operation);
uint32_t ambient_model_v6_close(uint64_t operation);
uint32_t ambient_model_v6_summary(uint64_t memory,AmbientModelSummary *out);
uint32_t ambient_model_v6_feed(uint64_t memory,uint8_t *bytes,uint32_t capacity,uint32_t *length);
uint32_t ambient_model_v6_details(uint64_t memory,AmbientProposalTarget target,uint8_t *bytes,uint32_t capacity,uint32_t *length);
uint32_t ambient_model_v6_visibility(uint64_t memory,AmbientProposalTarget target,uint32_t visible);
uint32_t ambient_model_v6_clear_discussion(uint64_t memory,uint64_t task,uint64_t revision,uint64_t workspace_revision);
uint32_t ambient_model_v6_validate(uint64_t memory,AmbientProposalTarget target,uint32_t *kind);
uint32_t ambient_model_v6_copy(uint64_t memory,AmbientProposalTarget target,uint8_t *bytes,uint32_t capacity,uint32_t *length);
#ifdef AMBIENT_MODEL_TEST_SUPPORT
uint32_t ambient_model_test_dispatch(uint64_t operation,const uint8_t *token,uint32_t token_length,const AmbientPluginKey *keys,uint32_t key_count,AmbientText root,const uint8_t *url,uint32_t url_length,AmbientModelCompletion *out);
#endif
#ifdef __cplusplus
}
#endif
#endif
