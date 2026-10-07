// Copyright 2026 Salesforce, Inc. All rights reserved.
//! Field-Level Entitlement Filter — inbound Omni/Flex Gateway policy.
//!
//! Derives per-field sensitivity for a data product live from Informatica CDGC and,
//! on the response leg, projects each field for the *caller*: sensitive fields the
//! caller's clearance and declared purpose don't entitle them to are withheld —
//! masked, nulled, or dropped — before the response reaches the agent.
//!
//! The sensitivity map (one entry per governed column, flagged sensitive when its
//! Business Term's structured Security Level is in `sensitiveLevels` — falling back to
//! a description substring marker only for terms with no level) is fetched via the CDGC
//! Login→JWT→ccgf-searchv2 chain and cached (lazy refresh, single-flight). Governed
//! by the same `format:service` egress + `HttpClient` pattern as the sibling
//! metadata-injection and conformance-guard policies.
//!
//! The caller's clearance and purpose come from request headers by default, or —
//! when `clearanceClaim`/`purposeClaim` are configured — from claims in the caller's
//! Bearer JWT (decoded here, verified by an upstream JWT Validation policy). The
//! schema id can likewise come from `schemaIdClaim`. See `claims`.
//!
//! The caller-entitlement decision is **fail-closed**: absent or insufficient claims
//! withhold every sensitive field. The policy is fail-open only on its own CDGC
//! outage (no map → pass through), per `failOpenOnCdgcError`.
//!
//! It **inspects and rewrites the response body**, so its outcome rides in an
//! `_entitlement` annotation in the payload plus an `x-entitlement-filtered` header.
//! It handles JSON and single-message SSE `tools/call` results; whole-stream SSE
//! rewrites are out of scope.

mod cdgc;
mod claims;
mod entitlement;
mod generated;

use std::collections::BTreeSet;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Result};
use pdk::data_storage::{DataStorage, DataStorageBuilder, DataStorageError, StoreMode};
use pdk::hl::timer::Clock;
use pdk::hl::*;
use pdk::logger;
use serde::Deserialize;
use serde_json::{json, Map, Value};

use crate::cdgc::{nonce_from_time, CachedFieldMap, RefreshLock};
use crate::entitlement::{apply, parse_csv_set, parse_level_set, plan, CallerContext, EntitlementPolicy, GovernedField, MaskMode};
use crate::generated::config::Config;

const FIELD_CACHE_NAMESPACE: &str = "fef-fieldmap";
const REFRESH_LOCK_NAMESPACE: &str = "fef-refresh-lock";
const FIELD_CACHE_KEY_PREFIX: &str = "fef-fieldmap-";
const REFRESH_LOCK_KEY_PREFIX: &str = "fef-lock-";
const REFRESH_LOCK_TTL_SECONDS: i64 = 30;
const REFRESH_LOCK_TTL_MS: u32 = (REFRESH_LOCK_TTL_SECONDS as u32) * 1000;
const FIELD_MAP_STORE_MIN_TTL_MS: u64 = 30 * 24 * 60 * 60 * 1000;
const CAS_MAX_RETRIES: u32 = 3;
const DEFAULT_TIMEOUT_MS: i64 = 5_000;
// Six chained CDGC calls (login, jwt, file, columns, term-links, terms).
const CDGC_REFRESH_BUDGET_MS: i64 = 15_000;
const DEFAULT_REFRESH_INTERVAL_SECONDS: i64 = 86_400;
const SEARCH_PATH: &str = "/ccgf-searchv2/api/v1/search";
const CT_FLATFIELD: &str = "com.infa.odin.models.file.flat.FlatField";
const REL_TECH_GLOSSARY: &str = "com.infa.ccgf.models.governance.IClassTechnicalGlossaryBase";
// The structured IDMC "Security Level" classification on a Business Term
// (Public | Internal | Confidential | Restricted) — the primary sensitivity signal.
const ATTR_SECURITY_CLASS: &str = "com.infa.ccgf.models.governance.securityClassification";
const DEFAULT_SENSITIVE_LEVELS: &str = "confidential,restricted";
const DEFAULT_SENSITIVE_MARKER: &str = "confidential";
const DEFAULT_CLEARANCE_HEADER: &str = "x-dp-clearance";
const DEFAULT_PURPOSE_HEADER: &str = "x-dp-purpose";
const DEFAULT_CLEARED_LEVELS: &str = "restricted";
const DEFAULT_MASK_TOKEN: &str = "***";
/// Response header reporting how many sensitive fields were withheld for the caller.
const ENTITLEMENT_HEADER: &str = "x-entitlement-filtered";

#[derive(Deserialize)]
struct CdgcLoginResponse {
    #[serde(rename = "sessionId")]
    session_id: String,
    #[serde(rename = "orgId")]
    org_id: String,
}
#[derive(Deserialize)]
struct CdgcJwtResponse {
    jwt_token: String,
}

#[derive(Clone)]
struct Ctx {
    asset_id: String,
    clearance: Option<String>,
    purpose: Option<String>,
}

fn is_content_method(method: &str) -> bool {
    matches!(
        method,
        "tools/call" | "resources/read" | "prompts/get"
            | "message/send" | "message/stream" | "SendMessage" | "SendStreamingMessage"
    )
}

fn now_secs(clock: &Clock) -> i64 {
    clock.now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}
fn elapsed_ms(start: SystemTime, now: SystemTime) -> i64 {
    now.duration_since(start).map(|d| d.as_millis() as i64).unwrap_or(0)
}
fn next_call_timeout(per_call_ms: i64, elapsed: i64) -> Option<Duration> {
    let remaining = CDGC_REFRESH_BUDGET_MS - elapsed;
    if remaining <= 0 {
        return None;
    }
    Some(Duration::from_millis(per_call_ms.min(remaining).max(1) as u64))
}
fn field_map_store_ttl_ms(config: &Config) -> u32 {
    let refresh = config.refresh_interval_seconds.unwrap_or(DEFAULT_REFRESH_INTERVAL_SECONDS).max(0) as u64;
    refresh.saturating_mul(2).saturating_mul(1000).max(FIELD_MAP_STORE_MIN_TTL_MS).min(u32::MAX as u64) as u32
}
fn entitlement_policy(config: &Config) -> EntitlementPolicy {
    EntitlementPolicy {
        cleared_levels: parse_level_set(config.cleared_levels.as_deref(), DEFAULT_CLEARED_LEVELS),
        allowed_purposes: parse_csv_set(config.allowed_purposes.as_deref().unwrap_or("")),
        mask_mode: MaskMode::parse(config.mask_mode.as_deref().unwrap_or("mask")),
        mask_token: config.mask_token.clone().unwrap_or_else(|| DEFAULT_MASK_TOKEN.to_string()),
    }
}
fn mode_str(mode: MaskMode) -> &'static str {
    match mode {
        MaskMode::Mask => "mask",
        MaskMode::Nullify => "nullify",
        MaskMode::Drop => "drop",
    }
}

/// Authenticate to IDMC (Login → JWT). Returns (jwt, orgId).
async fn cdgc_auth(client: &HttpClient, config: &Config, clock: &Clock, start: SystemTime) -> Result<(String, String)> {
    let per_call = config.timeout.unwrap_or(DEFAULT_TIMEOUT_MS);
    let login_body = serde_json::to_vec(&json!({
        "username": config.cdgc_org_username, "password": config.cdgc_org_password,
    }))?;
    let t = next_call_timeout(per_call, elapsed_ms(start, clock.now())).ok_or_else(|| anyhow!("budget before Login"))?;
    let login_resp = client.request(&config.cdgc_login_url).path("/identity-service/api/v1/Login")
        .headers(vec![("Content-Type", "application/json")]).body(&login_body).timeout(t).post().await
        .map_err(|e| anyhow!("CDGC login failed: {e}"))?;
    if login_resp.status_code() >= 300 {
        return Err(anyhow!("CDGC login status {}", login_resp.status_code()));
    }
    let login: CdgcLoginResponse = serde_json::from_slice(login_resp.body()).map_err(|e| anyhow!("parse login: {e}"))?;
    let nonce = nonce_from_time(clock.now());
    let cookie = format!("USER_SESSION={}", login.session_id);
    let t = next_call_timeout(per_call, elapsed_ms(start, clock.now())).ok_or_else(|| anyhow!("budget before JWT"))?;
    let jwt_resp = client.request(&config.cdgc_login_url)
        .path(&format!("/identity-service/api/v1/jwt/Token?client_id=idmc_api&nonce={nonce}"))
        .headers(vec![("cookie", cookie.as_str()), ("IDS-SESSION-ID", login.session_id.as_str())])
        .timeout(t).get().await.map_err(|e| anyhow!("CDGC JWT failed: {e}"))?;
    if jwt_resp.status_code() >= 300 {
        return Err(anyhow!("CDGC JWT status {}", jwt_resp.status_code()));
    }
    let jwt: CdgcJwtResponse = serde_json::from_slice(jwt_resp.body()).map_err(|e| anyhow!("parse jwt: {e}"))?;
    Ok((jwt.jwt_token, login.org_id))
}

/// One ccgf-searchv2 Elasticsearch query. Returns the `hits.hits[]` array's `sourceAsMap`s.
async fn cdgc_search(
    client: &HttpClient, config: &Config, clock: &Clock, start: SystemTime,
    jwt: &str, org: &str, body: &Value,
) -> Result<Vec<Value>> {
    let per_call = config.timeout.unwrap_or(DEFAULT_TIMEOUT_MS);
    let authz = format!("Bearer {jwt}");
    let payload = serde_json::to_vec(body)?;
    let t = next_call_timeout(per_call, elapsed_ms(start, clock.now())).ok_or_else(|| anyhow!("budget before search"))?;
    let resp = client.request(&config.cdgc_search_url).path(SEARCH_PATH)
        .headers(vec![
            ("Authorization", authz.as_str()),
            ("X-INFA-ORG-ID", org),
            ("X-INFA-SEARCH-LANGUAGE", "elasticsearch"),
            ("Content-Type", "application/json"),
        ])
        .body(&payload).timeout(t).post().await
        .map_err(|e| anyhow!("CDGC search failed: {e}"))?;
    if resp.status_code() >= 300 {
        return Err(anyhow!("CDGC search status {}", resp.status_code()));
    }
    let v: Value = serde_json::from_slice(resp.body()).map_err(|e| anyhow!("parse search: {e}"))?;
    Ok(v.get("hits").and_then(|h| h.get("hits")).and_then(Value::as_array)
        .map(|a| a.iter().filter_map(|h| h.get("sourceAsMap").cloned()).collect())
        .unwrap_or_default())
}

fn s(map: &Value, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Catalog-driven sensitivity map: Login → JWT, then via ccgf-searchv2 resolve the
/// schema asset, enumerate its columns and their linked Business Terms, and build the
/// per-field map (name + sensitive[term Security Level, desc-marker fallback] + governing term).
async fn fetch_field_map(
    client: &HttpClient,
    config: &Config,
    clock: &Clock,
    schema_id: &str,
) -> Result<(Vec<GovernedField>, Option<String>, Option<String>)> {
    let start = clock.now();
    let (jwt, org) = cdgc_auth(client, config, clock, start).await?;
    let sens_marker = config.sensitive_marker.as_deref().unwrap_or(DEFAULT_SENSITIVE_MARKER).to_lowercase();
    let sens_levels = parse_level_set(config.sensitive_levels.as_deref(), DEFAULT_SENSITIVE_LEVELS);

    // 1. Resolve the schema asset → location + identity.
    let files = cdgc_search(client, config, clock, start, &jwt, &org, &json!({
        "from":0,"size":1,"query":{"bool":{"must":[
            {"terms":{"elementType":["OBJECT"]}},
            {"terms":{"core.identity":[schema_id]}}]}}
    })).await?;
    let file = files.into_iter().next().ok_or_else(|| anyhow!("schema asset '{schema_id}' not found"))?;
    let location = s(&file, "core.location").ok_or_else(|| anyhow!("schema asset has no core.location"))?;
    let file_name = s(&file, "core.name");
    let external_id = s(&file, "core.externalId");

    // 2. Enumerate columns (children of the schema location).
    let cols = cdgc_search(client, config, clock, start, &jwt, &org, &json!({
        "from":0,"size":1000,"query":{"bool":{
            "must":[{"terms":{"core.classType":[CT_FLATFIELD]}}],
            "filter":[{"terms":{"core.location::path_hierarchy.parent":[location]}}]}}
    })).await?;
    if cols.is_empty() {
        return Err(anyhow!("no columns found for schema asset '{schema_id}'"));
    }
    let col_ids: Vec<String> = cols.iter().filter_map(|c| s(c, "core.identity")).collect();

    // 3. Column → Business Term links.
    let rels = cdgc_search(client, config, clock, start, &jwt, &org, &json!({
        "from":0,"size":5000,"query":{"bool":{"must":[
            {"terms":{"elementType":["RELATIONSHIP"]}},
            {"terms":{"type":[REL_TECH_GLOSSARY]}},
            {"terms":{"core.sourceIdentity":col_ids}}]}}
    })).await?;
    let mut col_to_term: Map<String, Value> = Map::new();
    let mut term_ids: Vec<String> = Vec::new();
    for r in &rels {
        if let (Some(src), Some(tgt)) = (s(r, "core.sourceIdentity"), s(r, "core.targetIdentity")) {
            col_to_term.entry(src).or_insert(Value::String(tgt.clone()));
            if !term_ids.contains(&tgt) {
                term_ids.push(tgt);
            }
        }
    }

    // 4. Resolve the linked terms → name / Security Level / description (fallback).
    let mut terms: Map<String, Value> = Map::new(); // termId → {name, desc, level}
    if !term_ids.is_empty() {
        let tdocs = cdgc_search(client, config, clock, start, &jwt, &org, &json!({
            "from":0,"size":5000,"query":{"bool":{"must":[
                {"terms":{"elementType":["OBJECT"]}},
                {"terms":{"core.identity":term_ids}}]}}
        })).await?;
        for t in &tdocs {
            if let Some(id) = s(t, "core.identity") {
                terms.insert(id, json!({
                    "name": s(t, "core.name"),
                    "desc": s(t, "core.description").unwrap_or_default(),
                    "level": s(t, ATTR_SECURITY_CLASS).unwrap_or_default(),
                }));
            }
        }
    }

    // 5. Build the field map, one entry per column.
    let mut fields = Vec::new();
    for c in &cols {
        let (Some(id), Some(name)) = (s(c, "core.identity"), s(c, "core.name")) else { continue };
        let mut sensitive = false;
        let mut term_name: Option<String> = None;
        if let Some(Value::String(tid)) = col_to_term.get(&id) {
            if let Some(term) = terms.get(tid) {
                term_name = term.get("name").and_then(Value::as_str).map(str::to_string);
                // Primary: the term's structured Security Level classification.
                // Fallback (only when no level is set): the description substring marker.
                let level = term.get("level").and_then(Value::as_str).unwrap_or("").trim().to_lowercase();
                sensitive = if level.is_empty() {
                    let desc = term.get("desc").and_then(Value::as_str).unwrap_or("");
                    desc.to_lowercase().contains(&sens_marker)
                } else {
                    sens_levels.contains(&level)
                };
            }
        }
        fields.push(GovernedField { name, sensitive, term: term_name });
    }
    Ok((fields, file_name, external_id))
}

async fn read_cached<S: DataStorage>(store: &S, key: &str) -> Option<CachedFieldMap> {
    match store.get::<CachedFieldMap>(key).await {
        Ok(Some((c, _))) => Some(c),
        Ok(None) => None,
        Err(e) => {
            logger::warn!("fef: cache read failed: {e}");
            None
        }
    }
}
async fn write_cached<S: DataStorage>(store: &S, key: &str, entry: &CachedFieldMap) {
    for _ in 0..CAS_MAX_RETRIES {
        match store.get::<CachedFieldMap>(key).await {
            Ok(Some((_, v))) => match store.store(key, &StoreMode::Cas(v), entry).await {
                Ok(()) => return,
                Err(DataStorageError::CasMismatch) => continue,
                Err(e) => { logger::warn!("fef: persist failed: {e}"); return; }
            },
            Ok(None) => match store.store(key, &StoreMode::Absent, entry).await {
                Ok(()) => return,
                Err(DataStorageError::CasMismatch) => continue,
                Err(e) => { logger::warn!("fef: persist failed: {e}"); return; }
            },
            Err(e) => { logger::warn!("fef: read-before-persist failed: {e}"); return; }
        }
    }
}
async fn try_acquire_refresh_lock<S: DataStorage>(store: &S, key: &str, now: i64) -> Result<bool, DataStorageError> {
    let entry = RefreshLock { acquired_at: now };
    match store.store(key, &StoreMode::Absent, &entry).await {
        Ok(()) => Ok(true),
        Err(DataStorageError::CasMismatch) => match store.get::<RefreshLock>(key).await? {
            Some((existing, v)) => {
                if now - existing.acquired_at < REFRESH_LOCK_TTL_SECONDS { Ok(false) }
                else {
                    match store.store(key, &StoreMode::Cas(v), &entry).await {
                        Ok(()) => Ok(true),
                        Err(DataStorageError::CasMismatch) => Ok(false),
                        Err(e) => Err(e),
                    }
                }
            }
            None => match store.store(key, &StoreMode::Absent, &entry).await {
                Ok(()) => Ok(true),
                Err(DataStorageError::CasMismatch) => Ok(false),
                Err(e) => Err(e),
            },
        },
        Err(e) => Err(e),
    }
}

async fn get_field_map<S: DataStorage>(
    client: &HttpClient, config: &Config, clock: &Clock, map_store: &S, lock_store: &S, asset_id: &str,
) -> Option<CachedFieldMap> {
    let key = format!("{FIELD_CACHE_KEY_PREFIX}{asset_id}");
    let ttl = config.refresh_interval_seconds.unwrap_or(DEFAULT_REFRESH_INTERVAL_SECONDS).max(0);
    let now = now_secs(clock);
    let cached = read_cached(map_store, &key).await;
    if let Some(c) = &cached {
        if now - c.timestamp < ttl {
            return cached;
        }
    }
    let lock_key = format!("{REFRESH_LOCK_KEY_PREFIX}{asset_id}");
    if !try_acquire_refresh_lock(lock_store, &lock_key, now).await.unwrap_or(true) {
        return cached;
    }
    match fetch_field_map(client, config, clock, asset_id).await {
        Ok((fields, name, external_id)) => {
            let entry = CachedFieldMap { fields, name, external_id, timestamp: now };
            write_cached(map_store, &key, &entry).await;
            Some(entry)
        }
        Err(e) => {
            logger::warn!("fef: field-map refresh failed for '{asset_id}': {e}");
            if config.fail_open_on_cdgc_error.unwrap_or(true) { cached } else { None }
        }
    }
}

// ---- response body helpers ----

fn parse_rpc(text: &str, is_sse: bool) -> Option<Value> {
    if is_sse {
        for line in text.lines() {
            if let Some(rest) = line.trim_start().strip_prefix("data:") {
                if let Ok(v) = serde_json::from_str::<Value>(rest.trim()) {
                    return Some(v);
                }
            }
        }
        None
    } else {
        serde_json::from_str(text).ok()
    }
}
fn frame(rpc: &Value, is_sse: bool) -> String {
    let j = rpc.to_string();
    if is_sse { format!("event: message\ndata: {j}\n\n") } else { j }
}
fn nav<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = root;
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        cur = match cur {
            Value::Object(m) => m.get(seg)?,
            Value::Array(a) => a.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}
fn nav_mut<'a>(root: &'a mut Value, path: &str) -> Option<&'a mut Value> {
    let mut cur = root;
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        cur = match cur {
            Value::Object(m) => m.get_mut(seg)?,
            Value::Array(a) => a.get_mut(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}
/// Where the business payload lives in the response, so we can write it back.
/// MCP/A2A wrap it in a JSON-RPC envelope; a REST API returns it directly.
enum Place {
    Structured,     // result.structuredContent (MCP)
    Content(usize), // result.content[i].text as embedded JSON (MCP)
    Rest,           // the response body *is* the payload (REST/HTTP API)
}

/// True if `path` in the payload points at a record (object) or an array of them.
fn has_records(payload: &Value, path: &str) -> bool {
    match nav(payload, path) {
        Some(Value::Array(a)) => a.iter().any(|v| v.is_object()),
        Some(Value::Object(_)) => true,
        _ => false,
    }
}

async fn request_filter(request_state: RequestState, config: Rc<Config>) -> Flow<Option<Ctx>> {
    let hs = request_state.into_headers_state().await;

    // Opt-in JWT-claims source: only when a *Claim config is set do we decode the
    // caller's Bearer token. The token is only decoded here — an upstream JWT
    // Validation policy must verify it. A configured claim wins over the header;
    // absent config or absent claim falls back to the header (backward compatible).
    let jwt_claims = if config.schema_id_claim.is_some()
        || config.clearance_claim.is_some()
        || config.purpose_claim.is_some()
    {
        claims::decode_bearer_claims(hs.handler().header("authorization").as_deref())
    } else {
        None
    };
    let from_claim = |name: &Option<String>| -> Option<String> {
        claims::claim_str(jwt_claims.as_ref()?, name.as_deref()?)
    };

    let schema_header = config.schema_id_header.as_deref().unwrap_or("x-dp-schema-id").to_ascii_lowercase();
    let asset_id = from_claim(&config.schema_id_claim)
        .or_else(|| hs.handler().header(&schema_header).filter(|v| !v.trim().is_empty()))
        .unwrap_or_else(|| config.schema_id.clone());
    let clearance_header = config.clearance_header.as_deref().unwrap_or(DEFAULT_CLEARANCE_HEADER).to_ascii_lowercase();
    let purpose_header = config.purpose_header.as_deref().unwrap_or(DEFAULT_PURPOSE_HEADER).to_ascii_lowercase();
    let clearance = from_claim(&config.clearance_claim)
        .or_else(|| hs.handler().header(&clearance_header).filter(|v| !v.trim().is_empty()));
    let purpose = from_claim(&config.purpose_claim)
        .or_else(|| hs.handler().header(&purpose_header).filter(|v| !v.trim().is_empty()));
    let ct = hs.handler().header("content-type").unwrap_or_default();
    if ct.starts_with("application/json") && hs.method().as_str() == "POST" {
        let bs = hs.into_body_state().await;
        if let Ok(v) = serde_json::from_slice::<Value>(&bs.handler().body()) {
            match v.get("method").and_then(Value::as_str) {
                Some(m) if is_content_method(m) => {
                    return Flow::Continue(Some(Ctx { asset_id, clearance, purpose }));
                }
                Some(_) => return Flow::Continue(None), // non-content JSON-RPC → skip
                None => {}
            }
        }
    }
    // REST / non-JSON-RPC → still filter.
    Flow::Continue(Some(Ctx { asset_id, clearance, purpose }))
}

#[allow(clippy::too_many_arguments)]
async fn response_filter<S: DataStorage>(
    response_state: ResponseState,
    request_data: RequestData<Option<Ctx>>,
    config: Rc<Config>,
    client: Rc<HttpClient>,
    clock: Rc<Clock>,
    map_store: Rc<S>,
    lock_store: Rc<S>,
) {
    let ctx = match request_data {
        RequestData::Continue(Some(c)) => c,
        _ => return,
    };

    let cc = match get_field_map(&client, &config, &clock, &*map_store, &*lock_store, &ctx.asset_id).await {
        Some(c) if !c.fields.is_empty() => c,
        _ => return, // no governed map → pass through (fail-open on our own outage)
    };
    let fields = cc.fields.clone();
    let policy = entitlement_policy(&config);
    let caller = CallerContext { clearance: ctx.clearance.clone(), purpose: ctx.purpose.clone() };
    // The projection is a caller-level decision — computed before touching the body,
    // so the entitlement header can be stamped in the headers phase.
    let projection = plan(&fields, &caller, &policy);
    let records_path = config.records_path.as_deref().unwrap_or("");

    let hs = response_state.into_headers_state().await;
    let ct = hs.handler().header("content-type").unwrap_or_default();
    let is_sse = ct.contains("event-stream");
    if !is_sse && !ct.contains("json") {
        return;
    }
    hs.handler().set_header(ENTITLEMENT_HEADER, &projection.withhold.len().to_string());
    hs.handler().remove_header("content-length");
    hs.handler().remove_header("content-encoding");
    let bs = hs.into_body_state().await;
    let text = match String::from_utf8(bs.handler().body()) {
        Ok(t) => t,
        Err(_) => return,
    };

    let mut rpc = match parse_rpc(&text, is_sse) {
        Some(v) => v,
        None => return,
    };
    // MCP/A2A wrap the payload in a JSON-RPC envelope; a REST API returns the JSON
    // payload directly. Detect which, extract the payload, and remember where to
    // write it back.
    let is_rpc = is_sse
        || rpc.get("jsonrpc").is_some()
        || (rpc.get("result").is_some() && rpc.get("id").is_some());
    let (mut payload, place) = if is_rpc {
        // Only govern successful tool results.
        let result = match rpc.get("result") {
            Some(r) => r,
            None => return,
        };
        // structuredContent, else the first JSON content[].text.
        if let Some(sc) = result.get("structuredContent") {
            (sc.clone(), Place::Structured)
        } else if let Some(arr) = result.get("content").and_then(Value::as_array) {
            let mut found = None;
            for (i, item) in arr.iter().enumerate() {
                if item.get("type").and_then(Value::as_str) == Some("text") {
                    if let Some(t) = item.get("text").and_then(Value::as_str) {
                        if let Ok(v) = serde_json::from_str::<Value>(t) {
                            found = Some((v, i));
                            break;
                        }
                    }
                }
            }
            match found {
                Some((v, i)) => (v, Place::Content(i)),
                None => return,
            }
        } else {
            return;
        }
    } else {
        // REST / HTTP API: the whole response body is the payload.
        (rpc.clone(), Place::Rest)
    };

    if !has_records(&payload, records_path) {
        return;
    }

    // Apply the projection to every record. Entitled callers withhold nothing.
    let mut withheld_all: BTreeSet<String> = BTreeSet::new();
    if !projection.withhold.is_empty() {
        if let Some(node) = nav_mut(&mut payload, records_path) {
            match node {
                Value::Array(a) => {
                    for e in a.iter_mut() {
                        if let Some(o) = e.as_object_mut() {
                            withheld_all.extend(apply(o, &projection));
                        }
                    }
                }
                Value::Object(o) => {
                    withheld_all.extend(apply(o, &projection));
                }
                _ => {}
            }
        }
    }
    if !withheld_all.is_empty() {
        logger::info!(
            "fef: withheld {} field(s) asset={} entitled={} [{}]",
            withheld_all.len(), ctx.asset_id, projection.entitled,
            withheld_all.iter().cloned().collect::<Vec<_>>().join(",")
        );
    }

    // Self-describe the entitlement decision in-band.
    if let Value::Object(root) = &mut payload {
        root.insert("_entitlement".to_string(), json!({
            "entitled": projection.entitled,
            "clearance": ctx.clearance,
            "purpose": ctx.purpose,
            "mode": mode_str(projection.mode),
            "withheld": withheld_all.iter().cloned().collect::<Vec<_>>(),
            "assetId": ctx.asset_id,
            "name": cc.name,
            "externalId": cc.external_id,
            "source": "cdgc",
        }));
    }

    // Write the payload back where we found it, then reframe.
    match place {
        Place::Structured => {
            if let Some(r) = rpc.get_mut("result") {
                if let Some(obj) = r.as_object_mut() {
                    obj.insert("structuredContent".to_string(), payload.clone());
                    // keep the text mirror consistent if present
                    if let Some(arr) = obj.get_mut("content").and_then(Value::as_array_mut) {
                        if let Some(first) = arr.iter_mut().find(|i| i.get("type").and_then(Value::as_str) == Some("text")) {
                            first["text"] = Value::String(payload.to_string());
                        }
                    }
                }
            }
        }
        Place::Content(i) => {
            if let Some(item) = rpc.pointer_mut(&format!("/result/content/{i}/text")) {
                *item = Value::String(payload.to_string());
            }
        }
        Place::Rest => {
            // REST: the response body is the payload itself (no envelope to reframe).
            rpc = payload.clone();
        }
    }

    if let Err(e) = bs.handler().set_body(frame(&rpc, is_sse).as_bytes()) {
        logger::warn!("fef: set_body failed: {e:?}");
    }
}

fn launch_policy<S: DataStorage + 'static>(
    launcher: Launcher, config: Rc<Config>, client: Rc<HttpClient>, clock: Rc<Clock>,
    map_store: Rc<S>, lock_store: Rc<S>,
) -> impl std::future::Future<Output = Result<()>> {
    let cfg_req = config.clone();
    let filter = on_request(move |rs| {
        let c = cfg_req.clone();
        async move { request_filter(rs, c).await }
    })
    .on_response(move |rs, rd| {
        let c = config.clone();
        let cl = client.clone();
        let ck = clock.clone();
        let ms = map_store.clone();
        let ls = lock_store.clone();
        async move { response_filter(rs, rd, c, cl, ck, ms, ls).await }
    });
    async move { launcher.launch(filter).await.map_err(Into::into) }
}

#[entrypoint]
async fn configure(
    launcher: Launcher,
    Configuration(bytes): Configuration,
    client: HttpClient,
    storage_builder: DataStorageBuilder,
    clock: Clock,
) -> Result<()> {
    let config: Config = serde_json::from_slice(&bytes)
        .map_err(|err| anyhow!("Failed to parse configuration '{}'. Cause: {}", String::from_utf8_lossy(&bytes), err))?;
    let config = Rc::new(config);
    let client = Rc::new(client);
    let clock = Rc::new(clock);
    if config.distributed.unwrap_or(false) {
        let ms = Rc::new(storage_builder.remote(FIELD_CACHE_NAMESPACE, field_map_store_ttl_ms(&config)));
        let ls = Rc::new(storage_builder.remote(REFRESH_LOCK_NAMESPACE, REFRESH_LOCK_TTL_MS));
        launch_policy(launcher, config, client, clock, ms, ls).await
    } else {
        let ms = Rc::new(storage_builder.local(FIELD_CACHE_NAMESPACE));
        let ls = Rc::new(storage_builder.local(REFRESH_LOCK_NAMESPACE));
        launch_policy(launcher, config, client, clock, ms, ls).await
    }
}
