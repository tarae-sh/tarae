; Scopes

[
  (function_item)
  (struct_item)
  (enum_item)
  (union_item)
  (type_item)
  (trait_item)
  (impl_item)
  (closure_expression)
  (block)
] @local.scope

; Definitions

(function_item
  (parameters
    (parameter
      pattern: (identifier) @local.definition.variable.parameter)))

(closure_parameters (identifier) @local.definition.variable.parameter)

; tarae: the names a `let`, a `for` or an `if let` binds, and a typed closure parameter — what a name in the chat finds
; (`chatlink.rs`); highlighting leaves `@local.binding` alone, so a `let`'s uses keep their colour. Not captured yet: a
; `match` arm's bindings (a bare identifier there may be `None`, which `None` in prose would then find) and struct
; patterns (`let Foo { a, b }`, `if let Some(Foo { a, .. })`)
(let_declaration pattern: (identifier) @local.binding)
(let_declaration pattern: (tuple_pattern (identifier) @local.binding))
(let_condition pattern: (tuple_struct_pattern type: (_) (identifier) @local.binding))
(for_expression pattern: (identifier) @local.binding)
(for_expression pattern: (tuple_pattern (identifier) @local.binding))
(closure_parameters (parameter pattern: (identifier) @local.binding))

; References
(identifier) @local.reference
