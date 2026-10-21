#ifndef AMBIENT_INTEGRATION_H
#define AMBIENT_INTEGRATION_H
#include "ambient.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Integration ABI1 uses the existing memory3 handle. No callback or cross-language allocation.
 * All calls are synchronous off the UI actor. Caller-owned aligned buffers remain live,
 * disjoint and unmodified during each call. Errors preserve outputs. Small output capacity
 * fails before mutation/dispatch. JSON is bounded Core output only; mutation inputs are typed.
 * Status codes follow ambient_source.h (0..15). A dispatch outcome is an HTTP status or
 * 408 deadline, 409 identity conflict, 413 limit, 422 invalid representation, 499 cancelled,
 * 500 transport/local failure. No server error body is returned.
 * Kind values are fixed; profile exports accept only implemented kinds (GITHUB, JIRA, TODOIST)
 * and return INVALID_ARGUMENT for others. Core finds the kind of a profile or operation by its ID.
 * Switch exports accept all five kinds. Disabling stops reads, cancels this kind's operations
 * for good and clears dependent assistance; saved facts, profiles and keys stay. A disabled
 * kind refuses begin/step/window/preview/bind and saved-fact use with STALE (11). Setting the
 * same value changes nothing.
 * GITHUB has no scope: scope_excluded with GITHUB is INVALID_ARGUMENT. TODOIST's scope is the
 * account: Core writes it once into the profile on the first GET /user of a read (NULL before);
 * a pinned profile accepts only its own account.
 * Cancel requests termination without waiting for Store/network. Dispatch holds the job
 * until client/runtime cleanup actually returns; its loop ends when Core refuses the next
 * step, including after the adapter's request maximum. Release is BUSY until then. Abort is
 * only for an unused begin, before dispatch. Memory close cancels and accepted dispatch
 * retains the Store lease for cleanup and matching-generation 401 revocation.
 * Dispatch secret parts are borrowed only for provider authorization, never Core/SQLite/JSON/
 * model, each at most 16384 bytes. GITHUB: first = token (ASCII graphic 1..16384), second
 * MUST be {NULL, 0}. JIRA: first = email (ASCII graphic 1..254, no colon), second = API token
 * (ASCII graphic 1..16384). TODOIST: first = personal token (ASCII graphic 1..16384), second
 * MUST be {NULL, 0}. A wrong form for the operation's kind is INVALID_ARGUMENT before any network
 * request and aborts the unused operation.
 * GITHUB: fixed https://api.github.com authority, API version 2026-03-10 and four GET paths
 * built from the profile allowlist; the target is a canonical https://github.com PR URL.
 * JIRA: fixed https://api.atlassian.com/ex/jira/{CloudID}/rest/api/3 authority and three GET
 * paths; the Cloud ID is the exact profile scope; scope exclusion survives profile deletion.
 * TODOIST: fixed https://api.todoist.com/api/v1 authority and four GET paths (user, the selected
 * task, its project, its comments); at most 9 requests. A 429/503 body is read in memory only,
 * at most 64 KiB, for a retry deadline; it is never stored or returned.
 */
#define AMBIENT_INTEGRATION_LOCAL_PROJECTS 1u
#define AMBIENT_INTEGRATION_CODEX 2u
#define AMBIENT_INTEGRATION_GITHUB 3u
#define AMBIENT_INTEGRATION_JIRA 4u
#define AMBIENT_INTEGRATION_TODOIST 5u
typedef struct AmbientGitHubRepository { AmbientText owner; AmbientText repository; } AmbientGitHubRepository;
typedef struct AmbientIntegrationProfileRef { uint64_t id, settings_revision, credential_generation; } AmbientIntegrationProfileRef;
typedef struct AmbientIntegrationRead {
    uint64_t task_id, task_revision;
    AmbientIntegrationProfileRef profile;
    AmbientText target;
    int64_t session_expires_at;
} AmbientIntegrationRead;
typedef struct AmbientIntegrationBatch {
    uint64_t task_id, task_revision;
    uint32_t count, reserved;
    uint64_t evidence_ids[32];
} AmbientIntegrationBatch;
/* By kind. profiles output 1MiB: {"kind","instance" (32 lowercase hex),"profiles","exclusions"};
 * window/saved page 256KiB; preview 512KiB. used points to exact UTF8 length. */
uint32_t ambient_integration_v1_profiles(uint64_t, uint32_t, uint8_t*, uint32_t, uint32_t*);
uint32_t ambient_integration_v1_scope_excluded(uint64_t, uint32_t, AmbientText, uint32_t);
/* By kind 1..5. enabled output and input are the closed boolean tag 0 or 1. */
uint32_t ambient_integration_v1_enabled(uint64_t, uint32_t, uint32_t*);
uint32_t ambient_integration_v1_set_enabled(uint64_t, uint32_t, uint32_t);
uint32_t ambient_integration_v1_saved_metadata(uint64_t, uint32_t, uint64_t, int64_t, uint8_t*, uint32_t, uint32_t*);
/* Typed profile saves. GitHub: name and 0..32 repositories (owner/repository segments of
 * 1..100 ASCII letters, digits, '-', '_' or '.', no case-insensitive duplicates). */
uint32_t ambient_integration_v1_github_profile_save(uint64_t, uint64_t, uint64_t, AmbientText, const AmbientGitHubRepository*, uint32_t, AmbientIntegrationProfileRef*);
/* Jira: name and Cloud ID; the Cloud ID of an existing profile is immutable. */
uint32_t ambient_integration_v1_jira_profile_save(uint64_t, uint64_t, uint64_t, AmbientText, AmbientText, AmbientIntegrationProfileRef*);
/* Todoist: name only. The account scope is never an input: a new profile has none, and saving
 * an existing profile keeps the account Core stored, read under the same lock. */
uint32_t ambient_integration_v1_todoist_profile_save(uint64_t, uint64_t, uint64_t, AmbientText, AmbientIntegrationProfileRef*);
/* By profile reference. Core-first replacement/revocation; native must serialize secret changes. */
uint32_t ambient_integration_v1_credential_begin(uint64_t, uint64_t, uint64_t, AmbientIntegrationProfileRef*);
/* Native confirms actual secret deletion before finish, with this exact revoked profile ref. */
uint32_t ambient_integration_v1_profile_delete_finish(uint64_t, AmbientIntegrationProfileRef);
uint32_t ambient_integration_v1_begin(uint64_t, const AmbientIntegrationRead*, uint64_t*);
/* By operation ID. */
uint32_t ambient_integration_v1_dispatch(uint64_t, uint64_t, AmbientText, AmbientText, uint32_t*);
uint32_t ambient_integration_v1_cancel(uint64_t, uint64_t);
uint32_t ambient_integration_v1_abort(uint64_t, uint64_t);
uint32_t ambient_integration_v1_release(uint64_t, uint64_t);
uint32_t ambient_integration_v1_window(uint64_t, uint64_t, uint32_t, uint8_t*, uint32_t, uint32_t*);
uint32_t ambient_integration_v1_preview(uint64_t, uint64_t, const uint32_t*, uint32_t, uint8_t*, uint32_t, uint32_t*);
uint32_t ambient_integration_v1_bind(uint64_t, uint64_t, const uint32_t*, uint32_t, int64_t, AmbientIntegrationBatch*);
#ifdef AMBIENT_MODEL_TEST_SUPPORT
/* Test-only isolated loopback, never present in production Mach-O. */
uint32_t ambient_integration_v1_test_dispatch(uint64_t, uint64_t, AmbientText, AmbientText, AmbientText, uint32_t*);
#endif
#ifdef __cplusplus
}
#endif
#endif
