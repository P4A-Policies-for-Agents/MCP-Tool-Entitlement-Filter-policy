#!/usr/bin/env python3
"""
Agent simulation for the MCP Tool Response Field Entitlement Filter demo.

The upstream MCP server exposes TWO tools over two different data products:

  * get_products  → dim_product.csv   (records under `products`,  unit_cost is Confidential)
  * get_customers → dim_customer.csv  (records under `customers`, credit_limit is Confidential)

Each tool returns the SAME record to every caller. What each caller receives differs
only by *who they are*: the policy picks the CDGC schema for the called tool
(`toolSchemas`), derives per-field sensitivity from Informatica CDGC, then projects
each field against the caller's clearance + declared purpose (request headers).

  * analyst  (clearance=internal,   purpose=analytics)       → sensitive fields MASKED (***)
  * fraud    (clearance=restricted, purpose=fraud-detection) → sensitive fields VISIBLE

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

# tool name → (arguments, records key in the result, the Confidential field to show)
TOOLS = {
    "get_products": ({"catalog": "products"}, "products", "unit_cost"),
    "get_customers": ({"segment": "retail"}, "customers", "credit_limit"),
}

def call(tool, arguments, extra_headers):
    body = {"jsonrpc": "2.0", "id": 9, "method": "tools/call",
            "params": {"name": tool, "arguments": arguments}}
    headers = {"Content-Type": "application/json",
               "Accept": "application/json, text/event-stream",
               "Accept-Encoding": "identity", "mcp-session-id": "entitlement-demo"}
    headers.update(extra_headers)
    req = urllib.request.Request(GW, data=json.dumps(body).encode(), method="POST", headers=headers)
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
    try:
        rpc = json.loads(raw)
    except ValueError:
        return None, filtered, f"non-JSON response: {raw[:120]!r}"
    if "error" in rpc:
        return None, filtered, rpc["error"].get("message", rpc["error"])
    if rpc.get("result", {}).get("isError"):
        return None, filtered, rpc["result"].get("content", [{}])[0].get("text", "isError")
    try:
        payload = json.loads(rpc["result"]["content"][0]["text"])
    except Exception:
        payload = rpc.get("result", {})
    return payload, filtered, None


def show(label, tool, extra_headers):
    arguments, records_key, field = TOOLS[tool]
    p, filtered, err = call(tool, arguments, extra_headers)
    print(f"── {label} → {tool} ──")
    if err:
        print(f"  (tool call failed: {err})\n")
        return
    ent = p.get("_entitlement", {})
    records = p.get(records_key) or []
    value = records[0].get(field, "(absent)") if records else "(no records)"
    print(f"  x-entitlement-filtered header : {filtered}")
    print(f"  governing schema (assetId)    : {ent.get('assetId')}  ({ent.get('name')})")
    print(f"  entitled                      : {ent.get('entitled')}")
    print(f"  withheld fields               : {ent.get('withheld') or '(none)'}  (mode={ent.get('mode')})")
    print(f"  {field + ' the caller sees':<30}: {value}")
    print()


def main():
    print(f"🔐  MCP Tool Response Field Entitlement Filter  →  {GW}\n")
    print("Each tool is mapped to its own CDGC schema (toolSchemas). Per-field")
    print("sensitivity is derived live from CDGC. Same tools + same records for every")
    print("caller — only the caller's clearance + purpose (request headers) differ:\n")
    for tool in TOOLS:
        for label, headers in PERSONAS.items():
            show(label, tool, headers)

    print("The cleared fraud investigator sees the Confidential fields; the analyst gets")
    print("them masked, on both tools. Nothing was configured per field — sensitivity came")
    print("from Informatica CDGC, and the entitlement decision is fail-closed.")


if __name__ == "__main__":
    main()
