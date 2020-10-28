include "llvm/Option/OptParser.td"

class F<string name>: Flag<["--", "-"], name>;

def o: JoinedOrSeparate<["-"], "o">, MetaVarName<"<path>">,
  HelpText<"Path to file to write output">;

def print_map: F<"print-map">,
  HelpText<"Print a link map to the standard output">;
