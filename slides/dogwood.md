+++
title: Dogwood
footer: Seattle Systems
transition: none
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
October 2026
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
- **Open source:** Apache 2, use it in your own stuff.

```dogwood
permit(principal, action, resource)
when { context.input.amount < 1000 }
when formerly within 1h {
    Action::"Approve"::request{ approver: context.input.approver }
};
```

---

# Who is Behind Dogwood?

- Jean-Baptiste Tristan
- Joseph Tassarotti
- and a great and expanding team.

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

---

# Why Not Python?

- **Analyzability**. Easy to understand what programs mean.
- **Composability**. Easy to join programs together.
- **Abstraction Level**. Easy to make multiple compatible implementations.

---

# Implementation: Current open source

- Optimized for **local** with no cloud dependencies.
- State only lives on this one device.
- Doesn't scale super well over lots of events.

---

# Implementation: AgentCore Policy

- Cloud-side implementation in AWS.
- Multi-tenant, hosted, serverless, nothing to manage.
- Temporal policies apply across agent sessions, across time.
- Durable. Highly available. No state is lost if an agent dies.
- Scales across tenants, and events per tenant.

---

# AgentCore Policy, backed by Aurora DSQL

- Compile Dogwood to SQL
- Run the SQL in Aurora DSQL
- Durability, scalability, high availability *for free*.
- Strong consistency *for cheap*.
- DSQL scales to any request rate, any data volume.
- Temporal primitives map quite cleanly to SQL.

---
# Concurrency control

<!-- align: center -->
Scalability is limited by
```art accent="▌"
▐▓▓▓▓▌▐▓▓▓▓▌▐▓▌ ▐▓▌▐▓▓▓▓▌▐▓▌▐▓▌▐▓▓▓▓▌▐▓▓▓▓▌▐▓▓▓▓▌▐▓▌ ▐▓▌▐▓▓▓▓▌▐▓▌ ▐▓▌
▐▓▌   ▐▓▌▐▓▌▐▓▓▌▐▓▌▐▓▌   ▐▓▌▐▓▌▐▓▌▐▓▌▐▓▌▐▓▌▐▓▌   ▐▓▓▌▐▓▌▐▓▌    ▐▓▐▓▌ 
▐▓▌   ▐▓▌▐▓▌▐▓▐▓▐▓▌▐▓▌   ▐▓▌▐▓▌▐▓▓▓▓▌▐▓▓▓▓▌▐▓▓▓▌ ▐▓▐▓▐▓▌▐▓▌     ▐▓▌  
▐▓▌   ▐▓▌▐▓▌▐▓▌▐▓▓▌▐▓▌   ▐▓▌▐▓▌▐▓▐▓▌ ▐▓▐▓▌ ▐▓▌   ▐▓▌▐▓▓▌▐▓▌     ▐▓▌  
▐▓▓▓▓▌▐▓▓▓▓▌▐▓▌ ▐▓▌▐▓▓▓▓▌▐▓▓▓▓▌▐▓▌▐▓▌▐▓▌▐▓▌▐▓▓▓▓▌▐▓▌ ▐▓▌▐▓▓▓▓▌  ▐▓▌  

▐▓▓▓▓▌▐▓▓▓▓▌▐▓▌ ▐▓▌▐▓▓▓▓▓▌▐▓▓▓▓▌▐▓▓▓▓▌▐▓▌   
▐▓▌   ▐▓▌▐▓▌▐▓▓▌▐▓▌  ▐▓▌  ▐▓▌▐▓▌▐▓▌▐▓▌▐▓▌   
▐▓▌   ▐▓▌▐▓▌▐▓▐▓▐▓▌  ▐▓▌  ▐▓▓▓▓▌▐▓▌▐▓▌▐▓▌  
▐▓▌   ▐▓▌▐▓▌▐▓▌▐▓▓▌  ▐▓▌  ▐▓▐▓▌ ▐▓▌▐▓▌▐▓▌          
▐▓▓▓▓▌▐▓▓▓▓▌▐▓▌ ▐▓▌  ▐▓▌  ▐▓▌▐▓▌▐▓▓▓▓▌▐▓▓▓▓▌
```

---
# Concurrency control

<!-- align: center -->
What the hell is
```art accent="▌"
▐▓▓▓▓▌▐▓▓▓▓▌▐▓▌ ▐▓▌▐▓▓▓▓▌▐▓▌▐▓▌▐▓▓▓▓▌▐▓▓▓▓▌▐▓▓▓▓▌▐▓▌ ▐▓▌▐▓▓▓▓▌▐▓▌ ▐▓▌
▐▓▌   ▐▓▌▐▓▌▐▓▓▌▐▓▌▐▓▌   ▐▓▌▐▓▌▐▓▌▐▓▌▐▓▌▐▓▌▐▓▌   ▐▓▓▌▐▓▌▐▓▌    ▐▓▐▓▌ 
▐▓▌   ▐▓▌▐▓▌▐▓▐▓▐▓▌▐▓▌   ▐▓▌▐▓▌▐▓▓▓▓▌▐▓▓▓▓▌▐▓▓▓▌ ▐▓▐▓▐▓▌▐▓▌     ▐▓▌  
▐▓▌   ▐▓▌▐▓▌▐▓▌▐▓▓▌▐▓▌   ▐▓▌▐▓▌▐▓▐▓▌ ▐▓▐▓▌ ▐▓▌   ▐▓▌▐▓▓▌▐▓▌     ▐▓▌  
▐▓▓▓▓▌▐▓▓▓▓▌▐▓▌ ▐▓▌▐▓▓▓▓▌▐▓▓▓▓▌▐▓▌▐▓▌▐▓▌▐▓▌▐▓▓▓▓▌▐▓▌ ▐▓▌▐▓▓▓▓▌  ▐▓▌  

▐▓▓▓▓▌▐▓▓▓▓▌▐▓▌ ▐▓▌▐▓▓▓▓▓▌▐▓▓▓▓▌▐▓▓▓▓▌▐▓▌   ▐▓▓▓▓▓▌
▐▓▌   ▐▓▌▐▓▌▐▓▓▌▐▓▌  ▐▓▌  ▐▓▌▐▓▌▐▓▌▐▓▌▐▓▌   ▐▓▌ ▐▓▌
▐▓▌   ▐▓▌▐▓▌▐▓▐▓▐▓▌  ▐▓▌  ▐▓▓▓▓▌▐▓▌▐▓▌▐▓▌      ▐▓▌ 
▐▓▌   ▐▓▌▐▓▌▐▓▌▐▓▓▌  ▐▓▌  ▐▓▐▓▌ ▐▓▌▐▓▌▐▓▌          
▐▓▓▓▓▌▐▓▓▓▓▌▐▓▌ ▐▓▌  ▐▓▌  ▐▓▌▐▓▌▐▓▓▓▓▌▐▓▓▓▓▌   ▐▓▌ 
```

---

# Concurrency control

- Decisions must be *atomic* and *isolated*
- Multiple concurrent agents could be referring to the *same policy*

```art accent="█"
 ░▒▓██████▓▒░ ░▒▓██████▓▒░░▒▓█▓▒░▒▓███████▓▒░  
░▒▓█▓▒░░▒▓█▓▒░▒▓█▓▒░░▒▓█▓▒░▒▓█▓▒░▒▓█▓▒░░▒▓█▓▒░ 
░▒▓█▓▒░░▒▓█▓▒░▒▓█▓▒░      ░▒▓█▓▒░▒▓█▓▒░░▒▓█▓▒░ 
░▒▓████████▓▒░▒▓█▓▒░      ░▒▓█▓▒░▒▓█▓▒░░▒▓█▓▒░ 
░▒▓█▓▒░░▒▓█▓▒░▒▓█▓▒░      ░▒▓█▓▒░▒▓█▓▒░░▒▓█▓▒░ 
░▒▓█▓▒░░▒▓█▓▒░▒▓█▓▒░░▒▓█▓▒░▒▓█▓▒░▒▓█▓▒░░▒▓█▓▒░ 
░▒▓█▓▒░░▒▓█▓▒░░▒▓██████▓▒░░▒▓█▓▒░▒▓███████▓▒░                                       
```

---

# Sell Only Once, With Approval

```dogwood
permit ( principal, action == Action::"SellShares", resource )
when temporal {
  formerly within 1h Action::"ApproveSale"::response{
    ...
  }
  && previous within 1h (
    !Action::"SellShares"::request{
      input.stock:  context.input.stock,
      input.shares: context.input.shares
    }
    since within 1h
    Action::"ApproveSale"::response{
      ...
      output.approved: true
    }
  )
};
```

---

# What Could Happen At Read Committed?

- **Read Committed** allows Lost Writes
- Agent A gets approval to sell
- Agent A and Agent B start selling the shares in parallel
- Reads say both should be allowed
- Both *COMMIT* (lost write)
- Both say *OK*

---

# Snapshot Isolation is Sufficient

- **Read Committed** allows Lost Writes
- Agent A gets approval
- Agent A and Agent B start selling the shares in parallel
- Reads say both should be allowed
- Both try to *COMMIT*
- B gets **committed**, A gets **aborted**
- Only one says *OK*

---

# Serializable vs Snapshot Isolation

- Our Dogwood implementation is serializable at the SI level
- Because each approval is an append to a per-session *logical log*
- So all concurrent transactions cause *write-write conflicts*

Yay for logs!

---

# Metric First-Order Temporal Logic

<!-- align: center -->
What the hell is
```art
 _______  _______  _______ _________ _       
(       )(  ____ \(  ___  )\__   __/( \      
| () () || (    \/| (   ) |   ) (   | (      
| || || || (__    | |   | |   | |   | |      
| |(_)| ||  __)   | |   | |   | |   | |      
| |   | || (      | |   | |   | |   | |      
| )   ( || )      | (___) |   | |   | (____/\
|/     \||/       (_______)   )_(   (_______/
                                             
```

---

# Temporal Logic

You know logic (and, or, xor, etc), right?

Some new operators about the future
<!-- align: center -->
*□*p (always), *◇*p (eventually), *○*p (at the next step), or p *U* q (p until q)

<!-- align: left -->
and the past

<!-- align: center -->
 *◆*p (once), *●*p (previous step), p *S* q (p since q)

---

# First-Order Logic

More new operators!

<!-- align: center -->
*∀*x (for all)
*∃*x (there exists)

e.g.
*∀*u. Slides(u) → *∃*d. Typo(u, d)

---

# Metric First-Order Temporal Logic

- Adds quantitative time constraints
- e.g. *in the last hour*, *since 1PM*

**◆[0,1h] ∃s. Approve(u, s)** becomes
```dogwood
formerly within 1h Action::"Approve"::request{
  input.user:  context.input.user,   // u: the current request's user
  input.stock: _                     // ∃s: any value
}
```

---

# Why, though?

- Temporal logic gives us a *firm mathematical foundation*
- Metric temporal logic matches *how our customers talk about time*
- A firm foundation helps with **analysis** and **composition**

---

# Try it out!

- https://github.com/dogwood-policy/dogwood (open source, Apache 2)
- Or in AgentCore Policy
- And more places coming soon
- You can use dogwood in your own stuff (please do!)

