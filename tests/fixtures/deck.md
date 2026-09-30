+++
title: Dogwood
footer: Seattle Systems
transition: push
+++

<!--
Example deck for tui-slides. Comments that are not directives, like this one,
are ignored, so they are a good place for speaker notes.
-->

# Dogwood
<!-- align: center -->

```art accent="●"
      ▄▀▀▄▄▀▀▄
      ▀▄    ▄▀
▄▀▀▀▄▄  ▀▄▄▀  ▄▄▀▀▀▄
█     ▀▀▄●●▄▀▀     █
▀▄▄▄▀▀  ▄▀▀▄  ▀▀▄▄▄▀
      ▄▀    ▀▄
      ▀▄▄▀▀▄▄▀
```

```art accent="╔╗╚╝═║" accent-color=lightcyan
██████╗  ██████╗  ██████╗ ██╗    ██╗ ██████╗  ██████╗ ██████╗
██╔══██╗██╔═══██╗██╔════╝ ██║    ██║██╔═══██╗██╔═══██╗██╔══██╗
██║  ██║██║   ██║██║  ███╗██║ █╗ ██║██║   ██║██║   ██║██║  ██║
██║  ██║██║   ██║██║   ██║██║███╗██║██║   ██║██║   ██║██║  ██║
██████╔╝╚██████╔╝╚██████╔╝╚███╔███╔╝╚██████╔╝╚██████╔╝██████╔╝
╚═════╝  ╚═════╝  ╚═════╝  ╚══╝╚══╝  ╚═════╝  ╚═════╝ ╚═════╝
```

## Temporal policy for AI agents

Marc Brooker · Seattle Systems

---

# Agent safety is a box
<!-- transition: dissolve -->

```art accent="╔╗╚╝═║╟"
┌───────────────────────────┐
│  ┌──────────┐             │                       ┌──────────────┐
│  │  Agent   │       ┌─────┴───┐╔══════╗           │              │
│  └──────────┘       │ Gateway │║Policy╟───────────┤     Tool     │
│        ┌──────────┐ │         │║      ╟─────┐  ┌──┤              │
│        │  Agent   │ └─────┬───┘╚══════╝     │  │  └──────────────┘
│        └──────────┘       │                 │  │
└───────────────────────────┘                 │  │  ┌──────────────┐
                                              │  │  │              │
                                              └──┼──┤ Microservice │
┌───────────────────────────┐                    │  │              │
│  ┌──────────┐             │                    │  └──────────────┘
│  │  Agent   │       ┌─────┴───┐╔══════╗        │
│  └──────────┘       │ Gateway │║Policy╟────────┘  ┌──────────────┐
│        ┌──────────┐ │         │║      ╟───────────┤              │
│        │  Memory  │ └─────┬───┘╚══════╝           │     SaaS     │
│        └──────────┘       │                       │              │
└───────────────────────────┘                       └──────────────┘
```

---

# What is Dogwood?

- **A governance language** for AI agents and the tools they call.
- **Cedar-derived:** familiar `permit` and `forbid`, with `when` and `unless`.
- **Temporal:** `since`, `formerly`, `once` and windowed aggregations look
  back over the agent's recent events.
- **Compiles to Cedar:** temporal facts become `context.*` slots, filled in
  when the request is evaluated.

```dogwood
permit(principal, action, resource)
when { context.input.amount < 1000 }
when formerly within 1h {
    Action::"Approve"::request{ approver: context.input.approver }
};
```

---

# Approve before you sell
<!-- Example from the Dogwood Playground: approve-before-sell. -->

An agent may sell only what a human approved, and only recently.

```dogwood
permit ( principal,
         action == AgentCore::Action::"SellShares", resource )
when temporal {
  formerly within 1h AgentCore::Action::"ApproveSale"::response{
    input.stock:     context.input.stock,
    input.shares:    context.input.shares,
    output.approved: true
  }
};
```

```art accent="✗" accent-color=lightred
  0m  SellShares   AMZN ×100    ✗ Deny    nothing approved yet
 28m  ApproveSale  AMZN ×100    → approved: true
 30m  SellShares   AMZN ×100    ✓ Allow
  2h  SellShares   AMZN ×100    ✗ Deny    the approval has expired
```

---

# An information barrier
<!-- Example from the Dogwood Playground: no-external-after-confidential. -->

Once the agent has read something confidential, nothing leaves.

```dogwood
permit ( principal,
         action == AgentCore::Action::"SendExternal", resource );

forbid ( principal,
         action == AgentCore::Action::"SendExternal", resource )
when temporal {
  formerly within 24h
    AgentCore::Action::"ReadConfidential"::response{}
};
```

```art accent="✗" accent-color=lightred
 0m  SendExternal      partner@example.com   ✓ Allow
 1m  ReadConfidential  roadmap
 2m  SendExternal      partner@example.com   ✗ Deny
```

---

# Test, then commit
<!--
The agent may commit only if the event immediately before the commit is a
passing `cargo test`. Checked against the Playground's Dogwood engine with
the trace in the timeline. Every event counts, including denied requests:
a refused commit is itself the previous event, so retrying needs another
test run first.
-->

`previous` looks at one event: the one just before this request.

```dogwood
permit ( principal,
         action == AgentCore::Action::"RunCommand", resource );

permit ( principal,
         action == AgentCore::Action::"GitCommit", resource )
when temporal {
  previous within 10m AgentCore::Action::"RunCommand"::response{
    input.command:    "cargo test",
    output.exit_code: 0
  }
};
```

```art accent="✗" accent-color=lightred
cargo test  ✓ exit 0                         git commit  ✓ Allow
cargo test  ✗ exit 101                       git commit  ✗ Deny
cargo test  ✓ exit 0    sed -i … lib.rs      git commit  ✗ Deny
cargo test  ✓ exit 0    … 11 minutes …       git commit  ✗ Deny
```

---

# Rate limits
<!-- Example from the Dogwood Playground: rate-limit. -->

`count_within` counts matching events in a sliding window, including
the request being decided.

```dogwood
permit ( principal,
         action == AgentCore::Action::"Transfer", resource );

forbid ( principal,
         action == AgentCore::Action::"Transfer", resource )
when temporal {
  count_within(1h,
    AgentCore::Action::"Transfer"::request{ input.amount: _ }) > 5
};
```

```art accent="✗" accent-color=lightred
Transfer $20, once a minute
 0m ✓   1m ✓   2m ✓   3m ✓   4m ✓   5m ✗  the sixth this hour
```

---

# One word is the bug
<!--
Examples from the Dogwood Playground: spend-limit-requests and
spend-limit-responses. The only difference is request vs response.
Both also have the baseline `permit` for Transfer shown on the rate limit slide.
-->

```dogwood
forbid ( principal,
         action == AgentCore::Action::"Transfer", resource )
when temporal {
  sum_within(a, 1h,
    AgentCore::Action::"Transfer"::request{ input.amount: a }
  ) > 5000
};
```

```art accent="✗" accent-color=lightred
                             ::request      ::response
   0s  Transfer $2000         ✓ Allow        ✓ Allow
   1s  Transfer $2000         ✓ Allow        ✓ Allow
   2s  Transfer $2000         ✗ Deny         ✓ Allow
 3-4s    two transfers settle
   5s  Transfer $2000         ✗ Deny         ✓ Allow
                              $4,000 out     $8,000 out
```

Sum **responses** and parallel transfers slip past the cap before any
of them settle.

---

# No sudden spikes
<!-- Example from the Dogwood Playground: anti-spike. -->

`bind` names a value from history so the current request can be
compared with it.

```dogwood
forbid ( principal,
         action == AgentCore::Action::"Transfer", resource )
when temporal {
  bind(prior,
    sum_within(a, 1h,
      AgentCore::Action::"Transfer"::response{ input.amount: a }),
    context.input.amount > prior)
};
```

```art accent="✗" accent-color=lightred
 0m  $1000 settles                  settled this hour: $1000
 1m  Transfer  $500   ✓ Allow       500 ≤ 1000
 2m  Transfer $2000   ✗ Deny        2000 > 1000
 3m  Transfer  $800   ✓ Allow       800 ≤ 1000
```
