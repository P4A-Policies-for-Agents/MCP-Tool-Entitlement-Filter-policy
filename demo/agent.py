#!/usr/bin/env python3
"""
Agent simulation for the Field-Level Entitlement Filter demo.

ONE upstream tool (`get_products`) returns the SAME product record every time —
including `unit_cost`, whose CDGC Business Term is marked Confidential. What each
caller receives differs only by *who they are*: the policy derives per-field
sensitivity from Informatica CDGC, then projects each field against the caller's
clearance + declared purpose (sent as request headers) and masks the fields the
caller isn't entitled to see.

  * analyst  (clearance=internal,   purpose=analytics)       → unit_cost MASKED (***)
  * fraud    (clearance=restricted, purpose=fraud-detection) → unit_cost VISIBLE

Same tool, same data, same gateway — the only difference is the caller's claims.

Usage:
    CMP_GW_URL="https://<host>/entitlement-filter-demo/mcp" python3 agent.py
"""
import json, os, ssl, sys, urllib.request, urllib.error

GW = (sys.argv[1] if len(sys.argv) > 1 else os.environ.get("CMP_GW_URL", "")).strip()
if not GW:
    sys.exit("Set CMP_GW_URL (governed endpoint or direct mock). See demo/env.local.sh.example")
_CTX = ssl.create_default_context(); _CTX.check_hostname = False; _CTX.verify_mode = ssl.CERT_NONE

PERSONAS = {
    "analyst (clearance=internal, purpose=analytics)":
        {"x-dp-clearance": "internal", "x-dp-purpose": "analytics"},
    "fraud investigator (clearance=restricted, purpose=fraud-detection)":
        {"x-dp-clearance": "restricted", "x-dp-purpose": "fraud-detection"},
}


def call(persona_headers):
    body = {"jsonrpc": "2.0", "id": 9, "method": "tools/call",
            "params": {"name": "get_products", "arguments": {"catalog": "products"}}}
    headers = {"Content-Type": "application/json",
               "Accept": "application/json, text/event-stream",
               "Accept-Encoding": "identity", "mcp-session-id": "entitlement-demo"}
    headers.update(persona_headers)
    req = urllib.request.Request(GW, data=json.dumps(body).encode(), method="POST", headers=headers)
    filtered = None
    try:
        resp = urllib.request.urlopen(req, timeout=25, context=_CTX)
        filtered = resp.headers.get("x-entitlement-filtered")
        raw = resp.read().decode()
    except urllib.error.HTTPError as e:
        filtered = e.headers.get("x-entitlement-filtered")
        raw = e.read().decode()
    for line in raw.splitlines():
        if line.startswith("data:"):
            raw = line[len("data:"):].strip(); break
    rpc = json.loads(raw)
    try:
        payload = json.loads(rpc["result"]["content"][0]["text"])
    except Exception:
        payload = rpc.get("result", {})
    return payload, filtered


def show(label, persona_headers):
    p, filtered = call(persona_headers)
    print(f"── {label} ──")
    ent = p.get("_entitlement", {})
    products = p.get("products") or []
    unit_cost = products[0].get("unit_cost") if products else "(no records)"
    print(f"  x-entitlement-filtered header : {filtered}")
    print(f"  entitled                      : {ent.get('entitled')}")
    print(f"  withheld fields               : {ent.get('withheld') or '(none)'}  (mode={ent.get('mode')})")
    print(f"  unit_cost the caller sees     : {unit_cost}")
    print()


def main():
    print(f"🔐  field-level entitlement filter  →  {GW}\n")
    print("Per-field sensitivity derived live from CDGC (unit_cost's Business Term")
    print("is marked Confidential). Same tool + same record for every caller —")
    print("only the caller's clearance + purpose (request headers) differ:\n")
    for label, headers in PERSONAS.items():
        show(label, headers)
    print("The cleared fraud investigator sees unit_cost; the analyst gets it masked.")
    print("Nothing was configured per field — sensitivity came from Informatica CDGC,")
    print("and the entitlement decision is fail-closed on missing/insufficient claims.")


if __name__ == "__main__":
    main()
