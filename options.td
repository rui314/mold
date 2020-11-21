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

def static: F<"static">, HelpText<"Do not link against shared libraries">;

defm trace_symbol: Eq<"trace-symbol", "Trace references to symbol">;

def trace: F<"trace">, HelpText<"Print the name of each input file">;

def filler: Separate<["--", "-"], "filler">;

defm library: Eq<"library", "Root name of library to use">,
  MetaVarName<"<libName>">;

defm library_path: Eq<"library-path", "Add a directory to the library search path">,
   MetaVarName<"<dir>">;

defm sysroot: Eq<"sysroot", "Set the system root">;

defm dynamic_linker: Eq<"dynamic-linker", "Which dynamic linker to use">;

def: JoinedOrSeparate<["-"], "l">, Alias<library>, HelpText<"Alias for --library">;
def: JoinedOrSeparate<["-"], "L">, Alias<library_path>, HelpText<"Alias for --library-path">;

def: F<"as-needed">;
def: F<"no-as-needed">;
