# Niao Language — VS Code Extension

Syntax highlighting and editor support for the [Niao](https://github.com/niao-lang/niao) programming language.

## Features

- Automatic detection of `.niao` source files
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
- Custom file icon for `.niao` files

## Requirements

- Visual Studio Code 1.64 or later (for language file icons)

## Installation

### From a `.vsix` package (local)

1. Build the extension (see [Build](#build) below).
2. In VS Code, open the Command Palette (`Ctrl+Shift+P` / `Cmd+Shift+P`).
3. Run **Extensions: Install from VSIX…**
4. Select `niao-language-0.1.0.vsix` from the `vscode-niao` folder.

### Development mode (F5)

1. Open the `vscode-niao` folder in VS Code.
2. Press `F5` to launch an Extension Development Host with Niao support loaded.

## Usage

Open any `.niao` file — VS Code will automatically use the Niao language mode.

Example:

```niao
fn greet(name: string) -> string {
    return "Hello, " + name
}

fn main() {
    print(greet("Niao"))
}
```

To manually set the language mode: click the language indicator in the status bar and choose **Niao**.

## Build

From the `vscode-niao` directory:

```bash
npm install
npm run package
```

This produces `niao-language-0.1.0.vsix` in the same folder.

### Prerequisites

- [Node.js](https://nodejs.org/) (LTS recommended)
- npm

## Test

1. Install dependencies and package the extension:

   ```bash
   cd vscode-niao
   npm install
   npm run package
   ```

2. Install locally:

   ```bash
   code --install-extension niao-language-0.1.0.vsix
   ```

3. Open a `.niao` file from the main Niao repository (e.g. `examples/hello.niao`) and verify highlighting.

Alternatively, press `F5` in VS Code with `vscode-niao` open to test without installing.

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
   cd vscode-niao
   npm install
   vsce publish
   ```

For a private or enterprise registry, use `vsce publish --registry <url>`.

## Extension structure

```
vscode-niao/
├── package.json                  # Extension manifest
├── language-configuration.json   # Comments, brackets, auto-close
├── syntaxes/
│   └── niao.tmLanguage.json      # TextMate grammar
├── icons/
│   └── niao-file-icon.svg        # File icon
├── .vscodeignore                 # Files excluded from .vsix
└── README.md
```

## Syntax reference

The grammar is derived from the official Niao lexer (`crates/niao_lexer`) and EBNF grammar (`docs/grammar.ebnf`).

| Element | Syntax |
|---------|--------|
| Comments | `//` line comments only |
| Strings | `"double quotes"` with `\n`, `\t`, `\\`, `\"` escapes |
| Integers | `42`, `0`, `10000` |
| Floats | `3.14`, `0.5` |
| Identifiers | `[a-zA-Z_][a-zA-Z0-9_]*` |

## License

MIT
