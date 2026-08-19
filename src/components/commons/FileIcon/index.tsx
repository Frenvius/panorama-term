import { Icon, addCollection } from '@iconify/react';
import catppuccin from '@iconify-json/catppuccin/icons.json';

addCollection(catppuccin);

const EXT_ICON: Record<string, string> = {
  ts: 'typescript',
  mts: 'typescript',
  cts: 'typescript',
  tsx: 'typescript-react',
  js: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  jsx: 'javascript-react',
  json: 'json',
  jsonc: 'json',
  json5: 'json',
  md: 'markdown',
  mdx: 'markdown-mdx',
  html: 'html',
  htm: 'html',
  css: 'css',
  scss: 'sass',
  sass: 'sass',
  less: 'less',
  vue: 'vue',
  svelte: 'svelte',
  astro: 'astro',
  py: 'python',
  pyc: 'python-compiled',
  ipynb: 'jupyter',
  rs: 'rust',
  toml: 'toml',
  yml: 'yaml',
  yaml: 'yaml',
  xml: 'xml',
  svg: 'svg',
  sh: 'bash',
  bash: 'bash',
  zsh: 'bash',
  fish: 'bash',
  ps1: 'powershell',
  psm1: 'powershell',
  bat: 'batch',
  cmd: 'batch',
  ahk: 'autohotkey',
  cs: 'csharp',
  csx: 'csharp',
  fs: 'fsharp',
  fsx: 'fsharp',
  vb: 'visual-studio',
  sln: 'visual-studio',
  csproj: 'msbuild',
  fsproj: 'msbuild',
  vbproj: 'msbuild',
  props: 'msbuild',
  targets: 'msbuild',
  nuspec: 'nuget',
  xaml: 'xaml',
  axaml: 'xaml',
  razor: 'razor',
  cshtml: 'razor',
  resx: 'xml',
  manifest: 'xml',
  c: 'c',
  h: 'c-header',
  cpp: 'cpp',
  cc: 'cpp',
  cxx: 'cpp',
  hpp: 'cpp-header',
  hh: 'cpp-header',
  hxx: 'cpp-header',
  asm: 'assembly',
  java: 'java',
  jar: 'java-jar',
  kt: 'kotlin',
  kts: 'kotlin',
  gradle: 'gradle',
  swift: 'swift',
  dart: 'dart',
  go: 'go',
  rb: 'ruby',
  gemspec: 'ruby-gem',
  php: 'php',
  lua: 'lua',
  luau: 'luau',
  r: 'r',
  jl: 'julia',
  scala: 'scala',
  ex: 'elixir',
  exs: 'elixir',
  erl: 'erlang',
  hs: 'haskell',
  nim: 'nim',
  zig: 'zig',
  pl: 'perl',
  pm: 'perl',
  ml: 'ocaml',
  mli: 'ocaml',
  clj: 'clojure',
  cljs: 'clojure',
  groovy: 'groovy',
  sql: 'database',
  db: 'database',
  sqlite: 'database',
  prisma: 'prisma',
  graphql: 'graphql',
  gql: 'graphql',
  proto: 'proto',
  tf: 'terraform',
  tfvars: 'terraform',
  hcl: 'terraform',
  bicep: 'bicep',
  nix: 'nix',
  vim: 'vim',
  tex: 'latex',
  diff: 'diff',
  patch: 'diff',
  wasm: 'web-assembly',
  exe: 'exe',
  dll: 'binary',
  bin: 'binary',
  txt: 'text',
  text: 'text',
  log: 'log',
  csv: 'csv',
  pdf: 'pdf',
  zip: 'zip',
  rar: 'zip',
  '7z': 'zip',
  tar: 'zip',
  gz: 'zip',
  ttf: 'font',
  otf: 'font',
  woff: 'font',
  woff2: 'font',
  mp3: 'audio',
  wav: 'audio',
  flac: 'audio',
  ogg: 'audio',
  mp4: 'video',
  mkv: 'video',
  mov: 'video',
  webm: 'video',
  ini: 'config',
  cfg: 'config',
  conf: 'config',
  properties: 'properties',
  env: 'env',
  lock: 'lock',
  pem: 'certificate',
  crt: 'certificate',
  cer: 'certificate',
  key: 'key',
  hbs: 'handlebars',
  ejs: 'ejs',
  pug: 'pug',
  twig: 'twig'
};

const IMAGE_EXTS = new Set(['png', 'jpg', 'jpeg', 'gif', 'webp', 'ico', 'bmp', 'avif']);

const iconForFile = (name: string): string => {
  const lower = name.toLowerCase();

  if (lower === '.gitignore' || lower === '.gitattributes' || lower === '.gitmodules') return 'git';
  if (lower === '.editorconfig') return 'editorconfig';
  if (lower === 'package.json' || lower === 'package-lock.json') return 'package-json';
  if (lower === 'bun.lock' || lower === 'bun.lockb') return 'bun';
  if (lower === 'readme' || lower === 'readme.md') return 'readme';
  if (lower === 'license' || lower === 'license.md' || lower === 'license.txt') return 'license';
  if (lower === 'changelog.md') return 'changelog';
  if (lower === 'contributing.md') return 'contributing';
  if (lower === 'codeowners') return 'codeowners';
  if (lower === 'robots.txt') return 'robots';
  if (lower === 'dockerfile' || lower === 'containerfile') return 'docker';
  if (lower === 'docker-compose.yml' || lower === 'docker-compose.yaml') return 'docker-compose';
  if (lower === '.dockerignore') return 'docker-ignore';
  if (lower === 'makefile' || lower === 'gnumakefile') return 'makefile';
  if (lower === 'cmakelists.txt' || lower.endsWith('.cmake')) return 'cmake';
  if (lower === 'go.mod' || lower === 'go.sum') return 'go-mod';
  if (lower.startsWith('.env')) return 'env';
  if (lower === 'tsconfig.json' || /^tsconfig\.[^.]+\.json$/.test(lower)) return 'typescript-config';
  if (lower.startsWith('vite.config.')) return 'vite';
  if (lower.startsWith('.eslintrc') || lower === 'eslint.config.js') return 'eslint';
  if (lower.startsWith('.prettierrc') || lower === 'prettier.config.js') return 'prettier';
  if (lower === 'cargo.toml' || lower === 'cargo.lock') return 'rust-config';
  if (lower === 'tauri.conf.json') return 'rust';

  const dot = lower.lastIndexOf('.');
  const ext = dot > 0 ? lower.slice(dot + 1) : '';

  if (IMAGE_EXTS.has(ext)) return 'image';
  return EXT_ICON[ext] ?? 'file';
};

interface FileIconProps {
  name?: string;
  dir?: boolean;
  open?: boolean;
  size?: number;
  className?: string;
}

const FileIcon = ({ name, dir, open, size = 14, className }: FileIconProps) => {
  const icon = dir ? (open ? 'folder-open' : 'folder') : iconForFile(name ?? '');
  return <Icon icon={`catppuccin:${icon}`} width={size} height={size} className={className} style={{ flex: 'none' }} />;
};

export default FileIcon;
