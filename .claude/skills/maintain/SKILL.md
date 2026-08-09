---
name: maintain
description: Sweep one maintenance lens over this repo, decide whether each measured gap is a real defect or a deliberate design decision, close exactly one, and open a PR — opening nothing when the lens is clean. Make sure to use this skill whenever a scheduled or weekly maintenance run asks to check mruby C API coverage and propose repairs, whenever someone asks what should be graduated to the typed beni surface next, and whenever the request is to sweep, audit, or close gaps reported by `rake api:priority` or recorded in `.api_coverage.yml` — even when the word "maintain" never appears. The invocation argument names the lens and defaults to `api-coverage`.
---

# Maintain

Nobody is waiting on the other end of this run. It fires on a schedule, decides on its
own whether there is anything worth doing, and **its most common correct outcome is to do
nothing and say so.**

That framing matters more than any single step below. A run that manufactures work to
justify itself is worse than a run that reports a clean lens: the repo carries the wrong
change forever, while the report costs one paragraph. Resist the pull to find something.

Close **exactly one** gap per run. Not one per lens, not one per PR round — one. A run
that finds five real gaps closes the first and names the other four in its report, so the
next run starts from a shorter list.

## The judgement this turns on

The repo can measure what is *not bound yet*. It cannot measure what *should be bound*.

That second question is answered by SPEC.md and CLAUDE.md, and they outrank the
measurement whenever the two disagree. Nearly every candidate a survey produces is a
deliberate decision rather than a defect — a symbol that stays in `sys` because no
wrapper can encode its invariant is *correctly* absent, however often mrbgems call it.

So treat the survey as a list of questions, never a list of tasks.

## Lenses

A lens is one thing the repo measures about itself, paired with the authority that says
what the measurement ought to be. The argument selects one; absent an argument, use
`api-coverage`.

| Lens | Survey command | Design authority |
|---|---|---|
| `api-coverage` (default) | `bundle exec rake "api:priority[40]"` | SPEC.md · CLAUDE.md Principle 11 · `.api_coverage.yml` |

Two things about the `api:*` family are easy to get wrong:

- **`api:surface` and `api:formats` are not surveys.** They already sit in `task default:`
  and run on every Stop hook, so they are green by construction and reveal nothing on a
  weekly cadence. They belong to the precondition check in step 1.
- **`api:coverage` renders, it does not detect.** It writes `docs/api_coverage.md` from
  the manifest. Only `api:priority` reports what is unbound.

## Workflow

### 1. Establish the ground before measuring

Confirm `main` is at its remote tip and that no *tracked* file has uncommitted changes —
a modified tracked file means a previous run left work behind, so stop and report that
rather than building on it. Untracked paths are not residue; note them and carry on.

Then run `bundle exec rake`. Running the repo's own gate *before* the survey does two
jobs that running it afterwards cannot:

- It proves the toolchain is staged. `api:coverage` reads the generated `bindings.rs`
  when an archive is present and **infers** the surface when it is not — so an unstaged
  environment yields a survey that looks authoritative and isn't.
- It settles priority. A red default task on a clean `main` outranks any coverage gap;
  when that happens it *becomes* this run's single item and the lens survey is skipped.

Then run the lens's survey command, and read `docs/api_coverage.md` for what the manifest
already claims.

### 2. Separate defects from decisions

Qualify the survey's top 40 and stop there. That bound is what keeps a run affordable on
a weekly cadence, and it costs nothing: the ranking already puts the load-bearing
candidates first, and anything below it will still be there next week. Say in the report
where you stopped, so a reader never mistakes a bounded pass for an exhaustive one.

Walk each candidate through the gate below, in order, stopping at the first source that
settles it. Record which source settled it and the file or commit that did — the report
needs those sentences more than the fix does.

1. **SPEC.md** — the source of truth. A behaviour SPEC describes but the crates lack is an
   implementation bug, and a real gap. SPEC's silence is not permission to build: it is
   the signal to extend SPEC first.
2. **CLAUDE.md Principle 11** — the graduation bar. An operation reaches the typed surface
   only when the wrapper can encode its invariant as a lifetime, a carrier, or a runtime
   check. A value that is VM-internal with no shape to add belongs in `sys` and is not a
   gap.
3. **`.api_coverage.yml`** — recorded means already graduated.
4. **`git log`** — the one thing the documents cannot tell you: whether this exact
   graduation was built and then withdrawn. Search the symbol across history
   (`git log --all -S<symbol>`) and read any `revert(...)` touching it. A withdrawn
   graduation stays withdrawn unless SPEC now says otherwise. Re-proposing one is the
   most expensive mistake available here, because it costs a full round to rediscover
   what the revert already learned.

Whatever the gate settles on, check the load-bearing claim in `vendor/mruby` before
acting on it. A commit message, a manifest note, and a SPEC sentence all state what
someone believed at the time; the vendored source states what mruby does. The repo's one
revert exists because a graduation was built on a premise nobody checked against
`class.c`, so this is the step where a shortcut costs the most.

Frequency ranks candidates; it never establishes eligibility. mrbgem C call frequency
systematically overstates the Rust-side gap — a symbol the C ecosystem leans on may be
one an embedder never touches.

### 3. Choose the one, by what a wrong binding costs the consumer

Order the survivors this way rather than by what is convenient to write:

1. Gaps that leave a consumer reasoning about memory, GC rooting, or exception state by
   hand. The typed surface exists precisely to make that unnecessary, so its absence is
   the most expensive kind of gap.
2. Gaps that force `unsafe` at a call site a carrier could make safe.
3. Everything else, frequency-ranked.

### 4. Write the intent sentence

Write one sentence naming the symbol, the Rust items that will bind it, the SPEC section
that authorizes it, and the files expected to change.

This step is what lets the round proceed without a human. `coding:inspect` asks the user
about whatever its target leaves unsettled, and there is nobody to answer — so the target
has to leave nothing unsettled. If the sentence cannot be written without asking a
question, the gap is ambiguous: stop and report the question rather than guessing an
answer nobody is present to correct.

A run that ends here is complete, not failed. Handing back the one question that would
unblock the work is worth more than a change made on a guessed answer, because the guess
is what a reviewer has to unpick later — and the question is the thing only this run was
in a position to find.

This sentence is also the round's confirmed work list, which is what makes the second
`coding:inspect` in step 5 behave as a drift check instead of an opening survey.

### 5. Run the round

Branch from `main` as `maintain/<lens>/<symbol>`, then:

1. `coding:inspect` with the intent sentence as its target — settle the scope against the
   code before writing anything.
2. Implement: `coding:write` when the change adds a capability to the typed surface,
   `coding:refactor` when it only restructures what is already there. Where SPEC is
   silent, extend SPEC first and implement against it.
3. Record the graduation in `.api_coverage.yml` — the C symbol and the public Rust item
   that binds it. A graduation the manifest does not record is unfinished, and the
   PostToolUse hook regenerates `docs/api_coverage.md` from this edit.
4. Run `bundle exec rake rust:verify`. This is not optional and cannot be delegated:
   `coding:inspect` reads, it never runs, so nothing before this point has proven the
   change compiles.
5. `coding:inspect` again with the same target. A confirmed work list is now in context,
   so this pass asks nothing and ends the run on drift or a fresh ambiguity.
6. `coding:refactor` over what was written, leaving nothing for the next reader to clean.
7. `git:commit`.

### 6. Check the budget

One PR, at most **5 files** and **500 changed lines** (additions plus deletions), measured
with `git diff --stat` against `main`.

Generated and lock files sit outside the budget and are excluded from the count:
`docs/api_coverage.md`, `Cargo.lock`, `Gemfile.lock`, `rbs_collection.lock.yaml`. The
first of those appears in *every* coverage PR by construction, since the hook regenerates
it whenever `.api_coverage.yml` is edited.

A change that will not fit is reported as too large for one round. Do not split it across
files to slip under the count — two halves that each compile but neither of which is
usable is exactly the outcome this budget exists to prevent.

### 7. Open the PR — or don't

Only a verified, in-budget change earns a PR:

| Outcome | Action |
|---|---|
| No eligible gap | Report and stop. No branch, no PR. |
| One gap closed and verified | Open the PR. |
| Drift, unresolved ambiguity, or failed verification | Stop, report what blocked it, leave the branch for a human. No PR. |

Title the PR as a Conventional Commit — release-please reads PR titles to decide the next
version, so a title outside the convention silently mis-versions the release. Write the
body as three things: the gap, the authority that established it was a defect rather than
a decision, and what a consumer can now do without reasoning about VM internals. Survey
output belongs in the report below, not in the PR.

Open it with `gh pr create --base main`.

## Report

Every run ends with this report, whether or not a PR exists. Its reader is deciding
whether to trust the run's judgement, so the rejections carry more weight than the fix —
they are where a wrong call would hide.

```markdown
# /maintain <lens> — <PR title, or "clean">

Gate: `bundle exec rake` <green|red> · Survey: <N> candidates · Eligible: <N>

| Candidate | Verdict | Settled by |
|---|---|---|
| `mrb_foo_bar` | taken | SPEC.md §… "the wrapper reads it through a carrier" — described, not implemented |
| `mrb_ci_baz` | decision | Principle 11 — call-frame index, no carrier to add |
| `mrb_qux` | withdrawn | `d04fb52` "withdraw owned rest-arg format projections" |

## Taken this round
<the intent sentence, the verification result, and the PR url — or where the run stopped>

## Left for next time
<eligible gaps not taken, in order>
```

When the lens is clean, the table is the whole report and "Taken this round" says so in
one line. State it plainly; a clean lens is the expected result, not a failure to find
work.
