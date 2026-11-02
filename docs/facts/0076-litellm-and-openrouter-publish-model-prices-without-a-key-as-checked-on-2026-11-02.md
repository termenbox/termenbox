---
id: 0076
title: LiteLLM and OpenRouter publish model prices without a key; LiteLLM repeats each model under many provider prefixes and differs from fact 0041 on one cache price
status: active
tags: [meter, cost, network]
date: 2026-11-02
kind: environmental
last-verified: 2026-11-02
expires: 2027-02-02
---

Checked on 2026-11-02 with plain unauthenticated `GET`s.

- **LiteLLM**: `https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json`, about 3.2 MB, 4,553 keys. Licence: MIT outside its `enterprise/` directory (the repository `LICENSE`). One JSON object keyed by model id; each value has `input_cost_per_token`, `output_cost_per_token`, and often `cache_read_input_token_cost` and `cache_creation_input_token_cost` (dollars per token, not per million), plus `_above_200k_tokens`, `_batches` and `_above_1hr` variants. The same model appears bare (`claude-sonnet-5-5`), with cloud prefixes (`anthropic.`, `us.`, `eu.`, `bedrock/…`, `vertex_ai/…`) and with reseller names; regional copies cost about 10% more. A `sample_spec` key is documentation, not a model.
- **OpenRouter**: `https://openrouter.ai/api/v1/models`, about 0.8 MB, 452 models. `{"data": [{"id": "anthropic/claude-sonnet-5.5", "pricing": {"prompt": "0.000002", "completion": "0.00001", "input_cache_read": "…", "input_cache_write": "…"}}]}`, strings in dollars per token. Variants carry a `:` suffix (`:batch`, `:free`); dots in versions.
- **Disagreement with fact 0041**: LiteLLM and OpenRouter list `claude-sonnet-5-5` cache read at $0.10 per million tokens (0.05 for batch); fact 0041 and the built-in default say $0.20. Anthropic's pricing page is the source of truth; check it before changing either.
- The terms of use of OpenRouter's API were not read beyond the endpoint answering without a key; the default source is therefore LiteLLM.

Used by decision 0195.
