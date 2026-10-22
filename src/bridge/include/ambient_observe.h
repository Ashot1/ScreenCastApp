#ifndef AMBIENT_OBSERVE_V1_H
#define AMBIENT_OBSERVE_V1_H
#include <stdint.h>
#include "ambient.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Background observation ABI1 (ADR-025). New beside memory3/model6; neither changes.
 * Core decides cadence, what is stored and sent, and checks and applies the answer;
 * native only captures, recognizes, submits and draws the highlight it is told.
 * All calls are synchronous off UI on the memory3 handle. Caller owns every buffer:
 * live, aligned, disjoint and unmodified until return; no pointer is retained, no
 * callback, no cross-language allocation. Errors leave outputs unchanged, except
 * status 6 of highlight/journal, which writes only the needed byte count to length.
 * Status 0 OK/1 invalid/2 paused/3 cancelled/4 busy/5 storage/6 limit/7 handle/8 panic
 * (as ambient_model.h). Dropped frames and blocked begins are results with status 0.
 * Time: at_utc is signed Unix seconds (history, the rolling hour); monotonic_ms is one
 * non-decreasing process clock of the caller (settling 10 s, backoff 5 min / 1 h). Pass
 * the same clock to every call; the finish inside dispatch continues it by elapsed time.
 * set_policy: the one JSON input, exactly {"started":bool,"per_hour_limit":1..60,
 * "credit_acknowledged":bool}, <=1024 bytes; started must equal credit_acknowledged
 * (D103). Stop cancels an observation in flight and clears images and highlight; the
 * text log stays for 24 h (D101).
 * submit_frame: frame of the display under the front window. strings holds all UTF-8 text
 * (<=8 MiB; NULL only at length 0); every span indexes it and may be empty. lines
 * (<=4096) own consecutive runs of words (<=16384) in order: the word_count values add up
 * to word_count. Rect: Vision normalized, origin bottom-left. jpeg 1..786432 bytes, kept
 * only in memory. reserved is 0 in inputs and outputs. Core validates content.
 * disposition 1 recorded/2 duplicate/3 not started/4 paused/5 cloud unavailable;
 * frame_id 0 unless recorded. next.capture 1 = act after next.seconds, 2 = hold (do not
 * capture; keep calling tick). Read highlight when highlight_version changed.
 * tick: the call without a capture while idle; app_switched 0/1 (an app switch asks for
 * an immediate capture). It also expires the day-old log.
 * begin: block 0 gives an operation (nonzero) for ambient_model_v6_dispatch/cancel/close
 * (one POST with the screenshots, no tools; the completion has purpose 3; close it
 * always). block 1 not due (not started, paused, not settled or significant, hourly
 * limit), 2 busy (the shared attempt slot), 3 cloud unavailable (session, backoff); then
 * operation is 0 and nothing needs closing. Status 6 while 64 operations are open (shared
 * with ambient_model_v6_begin). Cancel/close of an observation never touch
 * the manual assistance preview. After every observation dispatch read highlight.
 * highlight: Core JSON {"version":u64,"highlight":null|{"proposal_id","frame_id",
 * "display_key","anchor","rect":{x,y,width,height},"needs_recheck"}}; memory only.
 * journal: Core JSON {"policy","log_frames","unsent_frames","hour_attempts","sends":[
 * {"attempt","at_utc","first_frame","last_frame","apps","images","dropped_frames",
 * "dropped_bytes","dropped_images","outcome","tier","added_cards","rejected_items"}]},
 * newest first; at_utc first expires the day-old log. attempt is the completion's
 * attempt id. delete_log removes the text log and the send journal (not the attempt
 * accounting), cancels an observation in flight and clears images and highlight.
 */
typedef struct { double x,y,width,height; } AmbientObservedRect; /*32 bytes/alignment8*/
typedef struct { AmbientSpan text; AmbientObservedRect rect; } AmbientObservedWord; /*40/8*/
typedef struct { AmbientSpan text; uint32_t word_count; } AmbientObservedLine; /*12/4*/
typedef struct {
 AmbientText strings; AmbientSpan display_key,bundle_id,app_name,title;
 const AmbientObservedLine *lines; const AmbientObservedWord *words; AmbientText jpeg;
 uint32_t line_count,word_count,idle_seconds,reserved;
} AmbientObservedFrame; /*96 bytes/alignment8*/
typedef struct { uint32_t capture,seconds; } AmbientObserveNext; /*8 bytes*/
typedef struct {
 uint64_t frame_id,highlight_version; uint32_t disposition; AmbientObserveNext next; uint32_t reserved;
} AmbientObserveFrameResult; /*32 bytes/alignment8*/
typedef struct { uint64_t operation; uint32_t block,reserved; } AmbientObserveBegin; /*16 bytes*/
uint32_t ambient_observe_v1_set_policy(uint64_t memory,AmbientText policy);
uint32_t ambient_observe_v1_submit_frame(uint64_t memory,int64_t at_utc,uint64_t monotonic_ms,const AmbientObservedFrame *frame,AmbientObserveFrameResult *out);
uint32_t ambient_observe_v1_tick(uint64_t memory,int64_t at_utc,uint64_t monotonic_ms,uint32_t idle_seconds,uint32_t app_switched,AmbientObserveNext *out);
uint32_t ambient_observe_v1_begin(uint64_t memory,int64_t at_utc,uint64_t monotonic_ms,AmbientObserveBegin *out);
uint32_t ambient_observe_v1_highlight(uint64_t memory,uint8_t *bytes,uint32_t capacity,uint32_t *length);
uint32_t ambient_observe_v1_journal(uint64_t memory,int64_t at_utc,uint8_t *bytes,uint32_t capacity,uint32_t *length);
uint32_t ambient_observe_v1_delete_log(uint64_t memory);
#ifdef __cplusplus
}
#endif
#endif
