include "llvm/Option/OptParser.td"

class F<string name>: Flag<["--", "-"], name>;

multiclass Eq<string name, string help> {
  def NAME: Separate<["--", "-"], name>;
  def NAME # _eq: Joined<["--", "-"], name # "=">, Alias<!cast<Separate>(NAME)>,
    HelpText<help>;
}

def o: JoinedOrSeparate<["-"], "o">, MetaVarName<"<path>">,
  HelpText<"Path to file to write output">;

def print_map: F<"print-map">,
  HelpText<"Print a link map to the standard output">;

defm thread_count: Eq<"thread-count", "Number of threads to use">;

def stat: F<"stat">;
