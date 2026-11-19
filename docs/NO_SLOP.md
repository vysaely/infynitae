# No slop

The owner's rule for everything made in these projects (Infynitae, its edge and apps, Alyae,
Melodyae, the Aevyternae lab): every pixel, word and line of code is there because it does a job.
Work that looks finished but is padded, generic or invented is worse than less work done plainly,
because the owner reads it, trusts it and acts on it.

## Honesty and tests (first, because the rest depends on it)

- Tests are evidence that the code does what the brief asks; passing them is not the goal. If a
  test contradicts the brief or the documented behaviour, stop: name the test, say why it is
  wrong in the report, and change neither the code nor the test to make it pass.
- Reporting BLOCKED or PARTIAL with a wrong or impossible test named is good work, not failure.
- Ways of passing a test without doing the work, none of which is allowed: editing, skipping or
  `#[ignore]`-ing a test, deleting or loosening an assertion, special-casing the test's literal
  inputs, overriding equality or comparison, hidden counters or state that only a test reaches,
  exiting early, changing timeouts, the gate script, `.agy-gates` or CI config.
- A fix comes with a test that failed before it, and the report quotes that failing run.
- Never invent a path, function, config key, environment variable, CLI flag, crate, commit hash
  or result. Read the file before saying what it contains; otherwise write "not checked".
- Numbers come from a measurement or a source, named. "Tested" means the test ran and its output
  is quoted; otherwise "not tested".

## Code

- Every changed line traces to the brief. Do not reformat, rename or re-comment code you did not
  need to touch, and do not add docs or types to unchanged code. Remove scratch files.
- No speculative abstraction, trait, config option, feature flag or helper for a second use that
  does not exist. No dead code, no parameters kept "for later", no copy of a function with one
  line changed: reuse or change the one there is.
- Errors are handled where something can be done about them and reported plainly otherwise. No
  catch-all that swallows an error, no `unwrap_or_default` or `.ok()` that hides a real failure.
- No placeholder, dummy, mock or sample values in non-test code, and no fallback branch that keeps
  replaced code alive. A needed value that is missing goes in the report as a question.
- No checks for states that cannot happen; no retries, sleeps or timeouts added to hide a race.
- Mock only at real boundaries (network, hardware, the compositor, the clock), never the module
  under test; a small real fixture is better than a mock.
- No new dependency for something a few lines do. A new crate is named in the report with why;
  models suggest crates that do not exist, and attackers register those names.

## What people see (panels, apps, dialogs, notifications)

- Colours, corner radii, fonts and spacing come from the theme only (surface, card, text, muted,
  accent). The accent marks what is on, active or asks for attention.
- Nothing decorative that carries no information: no gradient washes, glows, extra shadows,
  emoji, status symbols, icons that repeat their own label, borders around every element.
- Generic layouts are the default to avoid, not to reach for: no grid of identical cards, ALL-CAPS
  labels above headings, "A · B · C" meta strings, "→" on every button, 01/02/03 on things that
  are not a sequence.
- Motion only where it shows a change of state; not a fade or slide on every element or hover.
- Real data or an honest empty state ("No app is playing sound"). Never sample entries, invented
  numbers, "Lorem ipsum", "Coming soon" or a placeholder left in.
- Buttons say what they do in one or two plain words (Add, Done, Cancel, Change). Labels are in
  sentence case, without marketing. Errors say what failed and what to do, without apology.
- One way to do a thing. Every control shown works.

## What people read (UI text, docs, comments, commit messages, reports)

- Say what it does or what happened, and why. Then stop: no introduction restating the task, no
  closing summary repeating it.
- Say what changed, with the measurement ("redraws once a minute, not every second"), instead of
  words like "improved", "robust", "seamless" or "powerful". The full word list is checked at
  review (`agy-task review`).
- Avoid these patterns too: "not X, but Y" contrasts, lists of three by reflex, a trailing
  "-ing" clause claiming significance ("…, ensuring smooth playback"), "serves as" for "is",
  and claims attributed to nobody ("experts say").
- Comments explain why something is the way it is, never narrate the next line. No
  commented-out code, no "TODO: improve" without what and why.
- Plain words before technical ones when the reader is the owner. Vary sentence length; short
  where it helps.

## The report

Before handing in, fill these in `AGY_REPORT.md`: test files touched and why; new dependencies;
new fallbacks or default values. "None" is a fine answer.
