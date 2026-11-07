export interface PluginSource {
  dir: string;
  manifest: string | null;
}

export interface PluginScan {
  root: string;
  plugins: PluginSource[];
}

export interface PluginManifest {
  id: string;
  name: string;
  version: string;
  description?: string;
  author?: string;
}

export type Plugin = { dir: string; manifest: PluginManifest; enabled: boolean } | { dir: string; error: string };
