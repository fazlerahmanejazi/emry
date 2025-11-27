; C tags query file

; Function definitions
(function_definition
  declarator: (function_declarator
    declarator: (identifier) @definition.function))

; Struct declarations
(struct_specifier
  name: (type_identifier) @definition.class)

; Enum declarations
(enum_specifier
  name: (type_identifier) @definition.enum)
