; Under the scissors line all is left out of the message: a comment whole (frictions #146 — past a `#` line's `:`,
; its text is the `diff` node's own, captured by nothing else), the diff layer's colours over it where it has some
[(comment) (generated_comment) (scissor) (diff)] @comment
(subject) @markup.heading
(branch) @string.special.symbol
(filepath) @string.special.path
(arrow) @punctuation.delimiter
(subject (subject_prefix) @function)
(prefix (type) @keyword)
(prefix (scope) @variable.parameter)
(prefix [ "(" ")" ":" ] @punctuation.delimiter)
(prefix "!" @punctuation.special)
(trailer (token) @variable.other.member)
(trailer (value) @string)
(breaking_change (token) @special)

(change kind: (new)) @diff.plus
(change kind: (deleted)) @diff.minus
(change kind: (modified)) @diff.delta
(change kind: [(renamed) (typechange)]) @diff.delta.moved
