#ifndef AMBIENT_PLUGIN_V2_H
#define AMBIENT_PLUGIN_V2_H
#include "ambient.h"
#ifdef __cplusplus
extern "C" {
#endif
/* Plugin ABI2 (ADR-021, ADR-022) replaces ABI1 without compatibility and uses the existing
 * memory3 handle. The bridge keeps no plugin state; its only table is of operation handles, like
 * the model's: operation_begin (<=64 open, else 6), operation_cancel (requests a stop; sticky,
 * so every discover takes a fresh operation), operation_close (4 while a call on it runs, until
 * its actual return including stopped processes). No callback or cross-language allocation. All calls are
 * synchronous off the UI actor; caller-owned aligned buffers remain live, disjoint and
 * unmodified during each call. Errors preserve outputs. Small output capacity fails (6) before
 * any mutation or network use. JSON is bounded Core output only; mutation inputs are typed.
 * Status codes follow ambient_source.h (0..15): stale revision/generation or a disabled plugin
 * 11, missing plugin/server/tool 14, limits 6, invalid input 1, busy operation 4, unknown
 * operation 7.
 * Package files: 1..64 records, kind 1 manifest (plugin.json or .codex-plugin/plugin.json),
 * 2 MCP servers (mcp.json or .mcp.json), 3 skill (skills/<dir>/SKILL.md); reserved 0; path
 * 1..1024 UTF-8 bytes relative to the plugin folder; bytes <=262144 each ({NULL, 0} = empty
 * file), <=1 MiB in total. Core parses and validates everything; native only reads bounded
 * files. Remote HTTPS servers to a named host and local commands are kept; package headers,
 * environment variable values and secrets are ignored (local variable names are read). Package
 * limits: command 1..1024 bytes, <=32 arguments <=1024 bytes, <=16 names ^[A-Z_][A-Z0-9_]{0,63}$
 * without PATH, HOME, TMPDIR, DYLD_*, LD_*; url together with command rejects the package; a
 * command inside the package folder is skipped with a warning. The package hash (64 lowercase hex) covers the files
 * sorted by path; install re-parses the same bytes and needs the reviewed hash (11 if not).
 * review needs no handle and writes {name,version,hash,servers,skills,warnings}, capacity
 * >=262144. list: {instance,plugins:[{id,revision,enabled,name,version,hash,installed_at_utc,
 * servers:[{key,kind,needs_key,downloads,generation}],skills,tools:[{server,name,hash}]}]},
 * where kind is {"kind":"remote",url,auth_required}, {"kind":"local",command,args,env (names)} or
 * {"kind":"managed","id":"google-workspace"} (a server Ambient installs; only the built-in package
 * may name it, an "ambient_managed" entry elsewhere is skipped with a warning) and downloads marks commands that fetch code at every start (npx, bunx, uvx, pnpm/yarn dlx,
 * pipx run); review servers carry the same {key,kind,needs_key,downloads}.
 * capacity >=1048576. calls: the newest <=256 call-log rows, capacity >=786432.
 * discover is blocking and takes an operation: Core picks the target of (plugin, server) at
 * exactly that generation; key is the bearer token (remote) or the compact JSON of variable
 * values (local) or the Google access token (managed), {NULL,0} when none. A remote server:
 * initialize + tools/list, at most 20 s. A local server is started for it in <root>/<plugin> (root
 * as in ambient_model.h), listed and stopped, at most 120 s; the managed server likewise on a
 * free 127.0.0.1 port, started within 30 s and listed within 20 s; cancel ends it at any point.
 * For the managed server only names in Core's allowlist are approvable. Writes {"status":"ok","tools":[{name,hash,
 * read_only,approvable,description,definition (approvable only)}]} or {"status":
 * "login_required"|"not_configured"|"unavailable"|"invalid"|"timeout"|"cancelled", "stderr"
 * (local failures only: the program's last <=8 KiB of stderr without its secret values)};
 * "stop_confirmed":false when the program's process group was not confirmed gone. Capacity
 * >=524288; nothing is stored. Loopback servers exist only in AMBIENT_MODEL_TEST_SUPPORT builds.
 * approve takes the exact canonical definition and its hash; Core re-hashes it and requires
 * readOnlyHint true and <=16384 bytes. A new pin keeps the revision, an identical pin changes
 * nothing, a changed pin and unapprove move the revision. Disabling moves the revision; enabling
 * does not; the same value changes nothing. revoke moves the server generation and the plugin
 * revision to one new value, returned in new_revision (the generation native writes into the
 * Keychain envelope). Removal: begin_removal disables and revokes every server at one value;
 * native deletes the Keychain entries; finish_removal deletes the plugin only at exactly that
 * revision while it is still disabled. Saved results stay in memory as history.
 * Keys and secrets never enter these calls except discover's borrowed key, which reaches only
 * the remote or managed server's Authorization header or the local program's environment, never
 * argv.
 * Managed server (ADR-022, 24B), id 1..64 bytes ("google-workspace"; unknown 1 before anything
 * runs). builtin writes the built-in package {"files":[{"kind":1|2,"path","text"}]} (UTF-8 bytes
 * exactly as hashed) for the ordinary review and install, capacity >=262144. managed_status is
 * stateless: {"status":"not_installed"|"installed"|"outdated"|"uv_missing"} (uv_missing only when
 * an install is needed), capacity >=1024. managed_install is blocking (<=10 min) on an operation:
 * the user's uv (/opt/homebrew/bin, /usr/local/bin, ~/.local/bin, ~/.cargo/bin) installs the
 * pinned arm64 requirements with required hashes into <root>/google-workspace-mcp/<sha256>.partial,
 * the import check runs, the folder is renamed and other versions are removed;
 * {"status":"installed"|"hash_mismatch"|"uv_missing"|"failed"|"cancelled"|"timeout",
 * "stderr_tail" (failures only, <=8 KiB)}, capacity >=65536. Removing the folder is native's.
 * OAuth (Google Desktop client built into the release; stateless, native holds state and
 * verifier of the one sign-in): oauth_begin(id, redirect "http://127.0.0.1:<port>/" exactly, else
 * 1) writes {"status":"ok","url","state","verifier"} or {"status":"unavailable"} (build without
 * the client); oauth_exchange(id, redirect, callback query <=8192, state, verifier) checks the
 * state and the exact issuer, exchanges the code with PKCE and the client secret and writes
 * {"status":"ok","access_token","refresh_token","expires_in"} or {"status":"denied"|"invalid"|
 * "failed"}; oauth_refresh(id, refresh token) writes {"status":"ok","access_token","expires_in"
 * [,"refresh_token" when rotated]} or {"status":"invalid_grant"|"failed"}; oauth_revoke(id, refresh
 * token) revokes the grant at the provider on sign-out and removal, best effort, and writes
 * {"status":"revoked"|"failed"} (capacity >=1024): native deletes its own key whatever it says.
 * Each token request is one POST of <=15 s without retry; capacity >=65536 unless stated. Tokens
 * are returned for the Keychain and never stored here.
 */
#define AMBIENT_PLUGIN_FILE_MANIFEST 1u
#define AMBIENT_PLUGIN_FILE_MCP 2u
#define AMBIENT_PLUGIN_FILE_SKILL 3u
typedef struct AmbientPluginFile { uint32_t kind; uint32_t reserved; AmbientText path; AmbientText bytes; } AmbientPluginFile; /*40 bytes/alignment8*/
uint32_t ambient_plugin_v2_review(const AmbientPluginFile *files,uint32_t count,uint8_t *bytes,uint32_t capacity,uint32_t *length);
uint32_t ambient_plugin_v2_install(uint64_t memory,const AmbientPluginFile *files,uint32_t count,AmbientText hash,int64_t at_utc,uint64_t *plugin);
uint32_t ambient_plugin_v2_list(uint64_t memory,uint8_t *bytes,uint32_t capacity,uint32_t *length);
uint32_t ambient_plugin_v2_calls(uint64_t memory,uint8_t *bytes,uint32_t capacity,uint32_t *length);
uint32_t ambient_plugin_v2_operation_begin(uint64_t *operation);
uint32_t ambient_plugin_v2_operation_cancel(uint64_t operation);
uint32_t ambient_plugin_v2_operation_close(uint64_t operation);
uint32_t ambient_plugin_v2_discover(uint64_t memory,uint64_t operation,AmbientText root,uint64_t plugin,AmbientText server,uint64_t generation,AmbientText key,uint8_t *bytes,uint32_t capacity,uint32_t *length);
uint32_t ambient_plugin_v2_approve(uint64_t memory,uint64_t plugin,uint64_t revision,AmbientText server,AmbientText definition,AmbientText hash,uint64_t *new_revision);
uint32_t ambient_plugin_v2_unapprove(uint64_t memory,uint64_t plugin,uint64_t revision,AmbientText server,AmbientText tool,uint64_t *new_revision);
uint32_t ambient_plugin_v2_set_enabled(uint64_t memory,uint64_t plugin,uint32_t enabled);
uint32_t ambient_plugin_v2_revoke(uint64_t memory,uint64_t plugin,uint64_t revision,AmbientText server,uint64_t *new_revision);
uint32_t ambient_plugin_v2_begin_removal(uint64_t memory,uint64_t plugin,uint64_t revision,uint64_t *new_revision);
uint32_t ambient_plugin_v2_finish_removal(uint64_t memory,uint64_t plugin,uint64_t revision);
uint32_t ambient_plugin_v2_builtin(AmbientText id,uint8_t *bytes,uint32_t capacity,uint32_t *length);
uint32_t ambient_plugin_v2_managed_status(AmbientText root,AmbientText id,uint8_t *bytes,uint32_t capacity,uint32_t *length);
uint32_t ambient_plugin_v2_managed_install(uint64_t operation,AmbientText root,AmbientText id,uint8_t *bytes,uint32_t capacity,uint32_t *length);
uint32_t ambient_plugin_v2_oauth_begin(AmbientText id,AmbientText redirect,uint8_t *bytes,uint32_t capacity,uint32_t *length);
uint32_t ambient_plugin_v2_oauth_exchange(AmbientText id,AmbientText redirect,AmbientText query,AmbientText state,AmbientText verifier,uint8_t *bytes,uint32_t capacity,uint32_t *length);
uint32_t ambient_plugin_v2_oauth_refresh(AmbientText id,AmbientText refresh,uint8_t *bytes,uint32_t capacity,uint32_t *length);
uint32_t ambient_plugin_v2_oauth_revoke(AmbientText id,AmbientText refresh,uint8_t *bytes,uint32_t capacity,uint32_t *length);
#ifdef __cplusplus
}
#endif
#endif
