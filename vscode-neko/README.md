# Neko Language — VS Code Extension

Syntax highlighting and editor support for the [Neko](https://github.com/neko-lang/neko) programming language.

## Features

- Automatic detection of `.neko` source files
- Syntax highlighting for:
  - Keywords (`fn`, `let`, `if`, `class`, `import`, …)
  - Type annotations (`int`, `float`, `string`, `bool`, `void`, `array`, `error`)
  - Literals (`true`, `false`, `nil`, numbers, double-quoted strings)
  - Line comments (`//`)
  - Operators (`+`, `==`, `&&`, `->`, `+=`, …)
  - HTTP route methods (`GET`, `POST`, `PUT`, `DELETE`, `PATCH`)
  - Built-in functions (`print`, `len`, `list_*`, `io_*`, `json_*`, …)
  - Function definitions, method calls, and member access
- Bracket matching and auto-closing pairs for `()`, `{}`, `[]`, and `""`
- Custom file icon for `.neko` files

## Requirements

- Visual Studio Code 1.64 or later (for language file icons)

## Installation

### From a `.vsix` package (local)

1. Build the extension (see [Build](#build) below).
2. In VS Code, open the Command Palette (`Ctrl+Shift+P` / `Cmd+Shift+P`).
3. Run **Extensions: Install from VSIX…**
4. Select `neko-language-0.1.0.vsix` from the `vscode-neko` folder.

### Development mode (F5)

1. Open the `vscode-neko` folder in VS Code.
2. Press `F5` to launch an Extension Development Host with Neko support loaded.

## Usage

Open any `.neko` file — VS Code will automatically use the Neko language mode.

Example:

```neko
fn greet(name: string) -> string {
    return "Hello, " + name
}

fn main() {
    print(greet("Neko"))
}
```

To manually set the language mode: click the language indicator in the status bar and choose **Neko**.

## Build

From the `vscode-neko` directory:

```bash
npm install
npm run package
```

This produces `neko-language-0.1.0.vsix` in the same folder.

### Prerequisites

- [Node.js](https://nodejs.org/) (LTS recommended)
- npm

## Test

1. Install dependencies and package the extension:

   ```bash
   cd vscode-neko
   npm install
   npm run package
   ```

2. Install locally:

   ```bash
   code --install-extension neko-language-0.1.0.vsix
   ```

3. Open a `.neko` file from the main Neko repository (e.g. `examples/hello.neko`) and verify highlighting.

Alternatively, press `F5` in VS Code with `vscode-neko` open to test without installing.

## Publish

To publish to the [Visual Studio Marketplace](https://marketplace.visualstudio.com/):

1. Create a [publisher](https://marketplace.visualstudio.com/manage) on the Marketplace.
2. Update `publisher` in `package.json` to your publisher ID.
3. Install vsce globally (if not using the local devDependency):

   ```bash
   npm install -g @vscode/vsce
   ```

4. Log in:

   ```bash
   vsce login <your-publisher-id>
   ```

5. Publish:

   ```bash
   cd vscode-neko
   npm install
   vsce publish
   ```

For a private or enterprise registry, use `vsce publish --registry <url>`.

## Extension structure

```
vscode-neko/
├── package.json                  # Extension manifest
├── language-configuration.json   # Comments, brackets, auto-close
├── syntaxes/
│   └── neko.tmLanguage.json      # TextMate grammar
├── icons/
│   └── neko-file-icon.svg        # File icon
├── .vscodeignore                 # Files excluded from .vsix
└── README.md
```

## Syntax reference

The grammar is derived from the official Neko lexer (`crates/neko_lexer`) and EBNF grammar (`docs/grammar.ebnf`).

| Element | Syntax |
|---------|--------|
| Comments | `//` line comments only |
| Strings | `"double quotes"` with `\n`, `\t`, `\\`, `\"` escapes |
| Integers | `42`, `0`, `10000` |
| Floats | `3.14`, `0.5` |
| Identifiers | `[a-zA-Z_][a-zA-Z0-9_]*` |

## License

MIT
