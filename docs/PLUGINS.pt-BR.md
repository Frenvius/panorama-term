# Plugins

*Read in [English](PLUGINS.md).*

Um plugin estende o Panorama pelos pontos de extensão que o app oferece. A maioria dos
plugins é declarativa: um `plugin.json` mais os arquivos que ele aponta, sem código.

Para um plugin completo, que usa todos os pontos de extensão (dois temas, duas fontes,
uma linguagem nova e uma associação de extensão), veja
[panorama-gatinhos](https://github.com/FlavioDias97/panorama-gatinhos).

## Onde os plugins ficam

Cada plugin é uma pasta dentro da pasta `plugins` do diretório de configuração do
Panorama:

| Sistema | Pasta |
| --- | --- |
| Linux | `~/.config/panorama/plugins/` |
| macOS | `~/Library/Application Support/panorama/plugins/` |
| Windows | `%APPDATA%\panorama\plugins\` |

As versões de desenvolvimento usam `panorama-dev` no lugar de `panorama`. **Settings →
Plugins** lista os plugins instalados, mostra a pasta e a abre. Mudanças na pasta
aparecem na hora, sem reiniciar. Plugins vêm ligados ao serem instalados e podem ser
desligados ali.

## O manifesto

```json
{
  "id": "nord",
  "name": "Nord",
  "version": "1.0.0",
  "description": "Arctic, north-bluish colors.",
  "author": "Alguém"
}
```

| Campo | Obrigatório | Regras |
| --- | --- | --- |
| `id` | sim | letras minúsculas, dígitos e hífens; igual ao nome da pasta |
| `name` | sim | aparece em Settings |
| `version` | sim | `maior.menor.correção`, com sufixo de pré-lançamento ou build opcional |
| `description` | não | aparece abaixo do nome |
| `author` | não | aparece quando não há descrição |

Um plugin sem manifesto, ou com manifesto inválido, aparece em Settings com o motivo.

`contributes` (opcional) lista o que o plugin acrescenta. Cada tipo está descrito abaixo.

## Temas

```json
{
  "contributes": {
    "themes": [
      {
        "id": "dark",
        "label": "Nord Dark",
        "kind": "dark",
        "colors": { "surface-1": "#2e3440", "text-1": "#d8dee9", "accent": "#88c0d0" },
        "terminal": {
          "foreground": "#d8dee9",
          "cursor": "#d8dee9",
          "ansi": ["#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
                   "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4"]
        }
      }
    ]
  }
}
```

| Campo | Obrigatório | Regras |
| --- | --- | --- |
| `id` | sim | letras minúsculas, dígitos e hífens; único dentro do plugin |
| `label` | sim | aparece em Settings; por convenção termina em "Dark" ou "Light" |
| `kind` | sim | `dark` ou `light` |
| `colors` | não | valores para as variáveis CSS do app, sem o `--` inicial |
| `terminal.foreground` / `terminal.cursor` | não | cor padrão do texto e do cursor |
| `terminal.ansi` | não | as 16 cores ANSI (normais 0-7, depois as brilhantes 8-15), usadas no texto e no fundo |

Um tema é aplicado por cima do tema embutido do mesmo tipo, então só precisa trazer o
que muda. **Settings → Appearance** mostra a escolha do tema escuro e do claro assim
que um plugin traz algum; *System* alterna entre os dois. Se o plugin for desligado ou
removido, o Panorama volta para o próprio tema.

Cores que um tema pode definir: `surface-1..3`, `border-1..3`, `text-1`, `text-strong`,
`text-muted`, `text-dim`, `accent`, `danger`, `run-glyph`, `run-fill`, `run-on-fill`,
`stop-fill`, `stop-on-fill`, `fill-hover`, `fill-press`, `hover-1..3`, `hover-soft`,
`pill-bg`, `minimap-bg`, `shadow-island`, `shadow-tile`, `shadow-pop`, `shadow-modal`,
e as cores de código `shiki-foreground`, `shiki-token-comment`, `shiki-token-punctuation`,
`shiki-token-link`, `shiki-token-keyword`, `shiki-token-string`,
`shiki-token-string-expression`, `shiki-token-constant`, `shiki-token-parameter`,
`shiki-token-function`. Qualquer outro nome torna o manifesto inválido.

## Fontes

```json
{
  "contributes": {
    "fonts": [
      {
        "family": "Iosevka Term",
        "monospace": true,
        "files": ["fonts/regular.woff2", { "path": "fonts/bold.woff2", "weight": 700 }]
      }
    ]
  }
}
```

| Campo | Obrigatório | Regras |
| --- | --- | --- |
| `family` | sim | letras, dígitos, espaços, pontos, hífens ou sublinhados; única dentro do plugin |
| `monospace` | não | `true` permite usar a fonte no código e no terminal; padrão `false` |
| `files` | sim | arquivos de fonte dentro da pasta do plugin (`.woff2`, `.woff`, `.ttf`, `.otf`) |
| `files[].weight` | não | `100` a `900`, padrão `400` |
| `files[].style` | não | `normal` ou `italic`, padrão `normal` |

Um arquivo pode ser só o caminho quando é a face regular. **Settings → Appearance**
mostra a escolha da fonte da interface, do código (editor e diff) e do terminal assim
que um plugin traz alguma. Só as fontes escolhidas são lidas, no momento da escolha. O
terminal mede a fonte escolhida, então as colunas continuam batendo com a largura do
tile. Se uma fonte não puder ser lida, ou o plugin for desligado, o Panorama volta para
a própria fonte. Os arquivos de fonte são lidos uma vez por sessão; trocá-los só vale
depois de reiniciar.

## Linguagens

```json
{
  "contributes": {
    "languages": [
      { "id": "mylang", "extensions": [".my"], "grammar": "syntaxes/mylang.tmLanguage.json" },
      { "id": "groovy", "extensions": [".jenkins"] }
    ]
  }
}
```

| Campo | Obrigatório | Regras |
| --- | --- | --- |
| `id` | sim | letras minúsculas, dígitos e `+ # . -`; único dentro do plugin |
| `extensions` | não | extensões de arquivo, como `.my`, que usam essa linguagem |
| `grammar` | não | uma gramática TextMate (`.json`, com `scopeName`) dentro do plugin |

Uma linguagem precisa de `grammar`, de `extensions` ou dos dois:

- **Com gramática**, o plugin traz uma linguagem que o Panorama não conhece. Arquivos
  com as extensões dela, e blocos de código marcados com o `id` dela, são coloridos com
  ela.
- **Sem gramática**, as extensões são associadas a uma linguagem que o Panorama já
  conhece (o `id` é essa linguagem, por exemplo `groovy`).

As cores seguem as cores de código do tema ativo, no editor, no diff e nos blocos de
código do Markdown. A gramática é lida na primeira vez que um arquivo precisa dela;
mudanças na gramática valem para os arquivos abertos depois.

## Limites

- Plugins só mudam o que os pontos de extensão expõem. Não conseguem trocar o estilo
  nem substituir partes do app.
- Plugins valem só no app desktop, não no app do celular.
- A fonte do terminal mantém o tamanho e a altura de linha do Panorama e é desenhada
  célula a célula, então precisa ser monoespaçada e não mostra ligaduras.
- Linguagens trazem só cor: nada de autocompletar, lint ou outros recursos de editor.
- No terminal, o branco ANSI (7) e o branco brilhante (15) chegam com a mesma cor,
  então um tema não consegue diferenciá-los; vale o branco brilhante.
