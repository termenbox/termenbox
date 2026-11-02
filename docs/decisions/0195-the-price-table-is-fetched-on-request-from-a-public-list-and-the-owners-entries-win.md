---
id: 0195-the-price-table-is-fetched-on-request-from-a-public-list-and-the-owners-entries-win
title: The model price table is fetched on request from a public list, kept apart from the owner's file, and a model with no price is counted, not free
status: draft
date: 2026-11-02
track: product
accepted-by: -
tags: [meter, cost, network, control-plane]
---

## Decision

The attention meter (decision 0165, plan step 1.4) prices a turn from a table of dollars per million tokens. The table has three layers, later ones winning per model: the built-in Anthropic list (fact 0041), a **fetched** table, and the owner's `prices.json`.

- **Opt-in.** The server fetches only on `--fetch-prices` (once, then exits) or, if the owner sets `TERMENBOX_FETCH_PRICES=on`, at most once a day while running. Without either, the server makes no call (this reverses the "no vendor calls" line in `attention_meter.rs` only for an owner who asks).
- **The request says nothing.** One `GET` of a fixed URL: no key, no body, no query, no cookie, and nothing about agents, models in use, usage or the owner. The only header sent is a `User-Agent` of `termenbox-server/<version>`.
- **Free and key-less sources only** (`no-paid-services`). The default is the LiteLLM `model_prices_and_context_window.json` (MIT); OpenRouter's public `/api/v1/models` is read in the same way. The URL is a setting (`--price-url`), the format is told from the content, so the owner can point at a mirror or at their own file.
- **The owner's file is never written.** The fetched table lives in `prices_fetched.json` beside the meter file, replaced atomically; `prices.json` stays the owner's and wins per model.
- **A failure keeps the old table.** Fetches are bounded (20 s, 8 MiB, https or loopback only); a body that does not parse, or prices fewer than 10 models, is dropped and the old file stays.
- **A gap is visible.** A turn on a model with no price (and no agent-reported cost) is counted as unpriced, per model and day, and the meter line says "N turns unpriced". It is not silently zero.
- **Ids are normalised** the same way when a table is built and when a model is looked up: lower case, provider prefix and `:variant` or `@version` suffix dropped, a trailing date dropped, dots written as dashes (`claude-sonnet-5.5` is `claude-sonnet-5-5`). Only a provider's own list price is taken: cloud-reseller and regional copies (which add a surcharge) are skipped.

## Why

A hand-written table cannot cover the hundreds of models people run through Codex, Claude Code and ACP agents, and an unlisted model costing nothing makes "$ per verified outcome" look better than it is. A public list fixes the coverage; keeping it in its own file means a refresh never overwrites a price the owner set on purpose. Making the call opt-in and content-free keeps the server's promise of calling nothing the owner did not ask for (fact 0071 on third-party terms is unaffected: this reads a price list, not a vendor account).

Not chosen: editing `prices.json` in place, because the owner's entries could not be told from fetched ones; a fetch at every start, because it is a surprise call; scraping vendor pricing pages, which have no stable format; shipping a larger built-in list, which goes stale on the next release.

## Consequences

- Prices are estimates from a third party's list, which can lag or be wrong (fact 0076 records a case). Agent-reported cost still wins over any estimate.
- The default source can disappear or change format; the meter then keeps its old table and the unpriced count shows the gap.
