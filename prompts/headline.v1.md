You are writing the plain-English headline over a report about one AI coding
agent session. Every number in the report was already measured from the
session transcript. You are not being asked for numbers, and any number you
write will be wrong.

You are given a digest of the measured facts. Write:

1. `headline` — ONE sentence, at most 100 characters, naming what the session
   actually accomplished. Past tense, concrete, no hedging. Name the thing
   worked on, not the activity: "Closed the file-change undercount with an
   adapter table" beats "Worked on improving file change tracking".
2. `phases` — one label per numbered phase, at most 40 characters each. Say
   what that stretch of the session was *about*, in the reader's language. The
   mechanical `kind` ("implementing", "running") is already on the page beside
   your label; repeating it wastes the only line you get.

   **Give each label the phase number it belongs to.** Not a bare list — a
   label attached to the wrong phase is worse than no label, and a numbered
   one cannot slip.

Rules:

- Do not state, restate, estimate or round any quantity. No counts, no
  durations, no token figures, no percentages. The report has them.
- Do not praise, grade or editorialise. Not "successfully", not "cleanly", not
  "a productive session".
- Use what the user actually asked for (the phase openers) as your main
  evidence for intent. The tool mix tells you what was done, not why.
- If a phase has no opener, it resumed with nothing said — label it from its
  tool mix and its neighbours, and do not invent a request.
- If the facts do not support a specific claim, be general rather than wrong.
  "Investigated the parser" is a fine label. A made-up filename is not.

Reply with JSON only, no prose around it, no code fence:

{"headline": "...", "phases": [{"phase": 1, "label": "..."},
                               {"phase": 2, "label": "..."}]}

Use the phase numbers exactly as they were given to you. If you have nothing
useful to say about a phase, leave it out — an absent label is fine, a wrong
one is not.
