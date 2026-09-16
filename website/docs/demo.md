# Demo

A static preview of what a walkthrough feels like. In the real extension this is live in your editor: the spotlighted statement is framed in amber, everything outside the dotted scope is dimmed, and hovering the highlight shows the step narrative and variable transitions.

**Step 3 of 6 — Validate stake covers debt** · `vault/LiquidationEngine.sol:41-44`

```solidity
contract LiquidationEngine {
    ┈┈ dimmed context ┈┈

    function liquidate(address vault, uint256 posId) external {
        Position storage p = positions[posId];
        uint256 price = oracle.price(vault);

        require(
            p.collateral * price >= p.debt * LIQ_THRESHOLD,
            "undercollateralized"
        );                                          // ← spotlight

        ┈┈ dimmed context ┈┈
    }
}
```

> **Variable** `p.collateral` — `validate` — `4.2e18 * price` → `>= debt * 1.1e18`
>
> **⚠ Security:** this is the only solvency check on the path. It reads `oracle.price` once and trusts it through both branches — a stale oracle lets an undercollateralized position through. Consider a freshness bound on the oracle round.

Transport: `[ ◀ prev ]` `Step 3/6` `[ next ▶ ]`

---

A recording of a full session is coming soon — along with the marketplace listing.
