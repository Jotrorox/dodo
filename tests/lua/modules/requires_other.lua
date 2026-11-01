-- fixture module that itself requires another module
local s = require("simplemod")
return {inner = s.name, greeting = s.greet("nested")}
