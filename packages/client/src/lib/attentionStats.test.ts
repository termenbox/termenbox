import { describe, expect, it } from 'vitest';
import type { AttentionDay, ClientMessage, ServerMessage } from './protocol';

// The same JSON the server's round-trip test in packages/core/src/protocol.rs writes and reads.
describe('attention meter messages', () => {
  it('card_seen carries the obligation id and milliseconds only', () => {
    const m: ClientMessage = { type: 'card_seen', obligation: 'a1', ms: 4200 };
    expect(JSON.parse(JSON.stringify(m))).toEqual({ type: 'card_seen', obligation: 'a1', ms: 4200 });
  });

  it('attention_stats parses, and a day from a newer server keeps the fields this client knows', () => {
    const wire = '{"type":"attention_stats","days":[{"day":20000,"raised":3,"by_layer":{"human":2,"rule":1},"by_device":{"phone":2},"human_ms":9000,"latency_ms":60000,"agent_ms":3600000,"later":true}]}';
    const m = JSON.parse(wire) as ServerMessage;
    expect(m.type).toBe('attention_stats');
    if (m.type !== 'attention_stats') return;
    const d: AttentionDay = m.days[0];
    expect([d.day, d.raised, d.by_layer.human, d.human_ms, d.agent_ms]).toEqual([20000, 3, 2, 9000, 3600000]);
  });
});
