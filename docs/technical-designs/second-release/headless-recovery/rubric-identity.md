# Identity redesign scoring rubric

Each criterion scores from 0 to 5. A viable design must score at least 3 on
every criterion and must not violate a fixed product or predecessor contract.

## 1. Identity correctness and atomicity

The external envelope carries an unambiguous formal template identifier. Every
file/document route checks the same identity before question validation or any
staged, flow, planning, target, or hook effect. Missing, malformed, and
mismatched identity have precise ordering and results.

## 2. Authority and interface depth

The design reuses the existing formal-name, canonical-target, configured-origin,
protocol, and interview authorities. Its interfaces make document verification
hard to bypass without moving transport concerns into the pure engine or adding
a second resolver/normalizer. The module hides meaningful policy behind a small
surface rather than forwarding data through shallow wrappers.

## 3. Route coherence and product reassessment

New apply, staged named apply, target-only apply, `continue`, terminal,
scripted, direct, staged replay, and public crate routes have one coherent
contract. The design re-evaluates each original recovery question under the
required identity rule and gives usable, concrete commands and documents.

## 4. Compatibility and predecessor composition

The design names the intentional break for bare maps and supplies truthful
migration guidance. It preserves non-terminal exit-4 JSON, the accepted flow
outcomes, CLI preview semantics, canonical target, origin-bearing configured
defaults, distinct errors, and in-memory `RawAnswers` use without inferring a
target/commit binding or another product restriction.

## 5. Proof quality and implementation readiness

The route matrix, error ordering, persistence/effect rules, public/private data
shapes, signatures, schema impact, documentation impact, failure injections,
and sole-kill guards are complete and falsifiable. The implementation can be
built without resolving a hidden product or ownership decision.
