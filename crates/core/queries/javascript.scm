; JavaScript tags query file

; Function declarations
(function_declaration
  name: (identifier) @definition.function)

; Method definitions
(method_definition
  name: (property_identifier) @definition.method)

; Class declarations
(class_declaration
  name: (identifier) @definition.class)
