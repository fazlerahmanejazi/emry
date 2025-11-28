; TypeScript tags query file

; Function declarations
(function_declaration
  name: (identifier) @definition.function)

; Method definitions
(method_definition
  name: (property_identifier) @definition.method)

; Class declarations
(class_declaration
  name: (identifier) @definition.class)

; Interface declarations
(interface_declaration
  name: (type_identifier) @definition.interface)

; Type alias declarations
(type_alias_declaration
  name: (type_identifier) @definition.type)

; Enum declarations
(enum_declaration
  name: (identifier) @definition.enum)
