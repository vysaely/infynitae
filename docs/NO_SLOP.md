# No slop

The owner's rule for everything made in these projects (Infynitae, its edge and apps, Melodyae,
the Aevyternae lab): every pixel, word and line of code is there because it does a job. Work
that looks finished but is padded, generic or invented is worse than less work done plainly.

## What people see (panels, apps, dialogs, notifications)

- Nothing decorative that carries no information: no gradient washes, glows, extra shadows,
  emoji, status symbols, icons that repeat their own label, borders around every element.
- The theme's tokens only (surface, card, text, muted, accent). The accent marks what is on,
  active or asks for attention; nothing else is coloured for colour's sake.
- Real data or an honest empty state ("nothing to show now", "No app is playing sound"). Never
  sample entries, invented numbers, "Lorem ipsum", "Coming soon" or a placeholder left in.
- Buttons say what they do in one or two plain words (Add, Done, Cancel, Change). Labels are
  sentences a person would say, in sentence case, without marketing ("Supercharge", "Smart",
  "Seamless", "Magic").
- One way to do a thing. No second toolbar, no setting for something nobody asked to change.
- Every control works. A button that does nothing yet is not shown.

## What people read (UI text, docs, comments, commit messages, reports)

- Say what it does or what happened, and why. Then stop. No introduction that restates the
  task, no closing summary that repeats it, no "In conclusion".
- No hype or filler words: seamless, robust, powerful, comprehensive, cutting-edge, blazing,
  elegant, leverage, utilize, delve, elevate, unlock, empower, game-changer, best-in-class,
  "it's worth noting", "simply", "just", "Here's the thing". "Improved" or "enhanced" only with
  what changed ("redraws once a minute, not every second").
- Numbers come from a measurement or a source, with where they came from. Not measured: say so.
  "Tested" means the test ran and its output is quoted; otherwise "not tested".
- Comments explain why something is the way it is, never narrate the next line. No commented-out
  code, no "TODO: improve" without what and why.
- Plain words before technical ones when the reader is the owner. One idea per sentence.

## Code

- Do what was asked. No speculative abstraction, trait, config option, feature flag or helper
  for a second use that does not exist.
- No dead code, no unused parameters kept "for later", no copy of a function with one line
  changed: reuse or change the one there is.
- Errors are handled where something can be done about them, and reported plainly otherwise.
  No catch-all that swallows an error, no `unwrap_or_default` that hides a real failure.
- No checks for states that cannot happen; no retries, sleeps or timeouts added to hide a race.
- A fix comes with a test that failed before it. A test is never weakened or deleted to pass.
- No new dependency for something a few lines do.

## The lab (Aevyternae)

- A result is reported with its guesses (what the shortcuts score) and its audit (her actual
  answers, by kind), on held-out data, and only as far as the test reaches.
- No "breakthrough", "she understands", "emergent" or "reasoning" claims beyond the measured
  scores. A failed or confounded test is written up as one, with the reason.

## How to check your own work before handing it in

Read it as the owner would. For each element, sentence and function ask: what job does this do,
and what would be lost without it? If the answer is "nothing", take it out.
