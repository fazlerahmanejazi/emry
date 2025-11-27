; C++ tags query file

; Function definitions
(function_definition
  declarator: (function_declarator
    declarator: (identifier) @definition.function))

; Class declarations
(class_specifier
  name: (type_identifier) @definition.class)

; Struct declarations
(struct_specifier
  name: (type_identifier) @definition.class)

; Enum declarations
(enum_specifier
  name: (type_identifier) @definition.enum)

; Namespace definitions
(namespace_definition
  name: (identifier) @definition.module)
