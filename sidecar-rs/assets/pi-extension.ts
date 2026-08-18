import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import {
	CustomEditor,
	type ExtensionAPI,
	type ExtensionContext,
	type Model,
	type ThinkingLevel,
} from "@earendil-works/pi-coding-agent";

const MOUSE_TRACKING = "\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1006h";

const AGENT_STATE = "\x1b]777;notify;panorama://agent-state;";

const CATALOG_MAX = 12000;

const THINKING_LEVELS: ThinkingLevel[] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

const CTRL_BACKSPACE = "\x08";
const DELETE_WORD_BACKWARD = "\x17";

const CONSOLE_SCRIPT = `
Add-Type -Namespace P -Name C -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError=true)] public static extern IntPtr GetStdHandle(int n);
[DllImport("kernel32.dll", SetLastError=true)] public static extern bool GetConsoleMode(IntPtr h, out uint m);
[DllImport("kernel32.dll", SetLastError=true)] public static extern bool SetConsoleMode(IntPtr h, uint m);
'@
$handle = [P.C]::GetStdHandle(-10)
$mode = 0
if (-not [P.C]::GetConsoleMode($handle, [ref]$mode)) { exit 1 }
$wanted = ($mode -band (-bnot 0x0040)) -bor 0x0080 -bor 0x0010
if (-not [P.C]::SetConsoleMode($handle, $wanted)) { exit 1 }
`;

function efforts(model: Model<any>): ThinkingLevel[] {
	if (!model.reasoning) return [];
	const map = model.thinkingLevelMap;
	return THINKING_LEVELS.filter((level) => !map || map[level] !== null);
}

function catalog(ctx: ExtensionContext) {
	const scoped = new Set(ctx.scopedModels.map((entry) => entry.model.id));
	const available = ctx.modelRegistry.getAvailable();
	const models = scoped.size > 0 ? available.filter((model) => scoped.has(model.id)) : available;
	return models.map((model) => ({
		id: model.id,
		provider: model.provider,
		contextWindow: model.contextWindow,
		efforts: efforts(model),
	}));
}

function announceCatalog(ctx: ExtensionContext): void {
	const models = catalog(ctx);
	// The sidecar drops an OSC payload past 16KB, which would take the whole announce with it.
	while (models.length > 0 && JSON.stringify(models).length > CATALOG_MAX) models.pop();
	announce({ models });
}

function announce(state: Record<string, unknown>): void {
	process.stdout.write(`${AGENT_STATE}${JSON.stringify(state)}\x07`);
}

class PanoramaEditor extends CustomEditor {
	handleInput(data: string): void {
		super.handleInput(data === CTRL_BACKSPACE ? DELETE_WORD_BACKWARD : data);
	}
}

interface PiSettings {
	tuiMode?: string;
	editorPaddingX?: number;
	autocompleteMaxVisible?: number;
}

function readSettings(path: string): PiSettings {
	try {
		return JSON.parse(readFileSync(path, "utf8")) as PiSettings;
	} catch {
		return {};
	}
}

function settingsChain(cwd: string): PiSettings[] {
	return [readSettings(join(cwd, ".pi", "settings.json")), readSettings(join(homedir(), ".pi", "agent", "settings.json"))];
}

function editorOptions(cwd: string) {
	const chain = settingsChain(cwd);
	return {
		paddingX: chain.find((s) => s.editorPaddingX !== undefined)?.editorPaddingX ?? 0,
		autocompleteMaxVisible: chain.find((s) => s.autocompleteMaxVisible !== undefined)?.autocompleteMaxVisible ?? 5,
	};
}

function usesFullscreen(cwd: string): boolean {
	const flag = process.argv.indexOf("--tui-mode");
	if (flag !== -1) return process.argv[flag + 1] === "fullscreen";

	return settingsChain(cwd).find((s) => s.tuiMode !== undefined)?.tuiMode === "fullscreen";
}

function enableConsoleMouse(): boolean {
	const encoded = Buffer.from(CONSOLE_SCRIPT, "utf16le").toString("base64");
	try {
		// The console handle only reaches the child through an inherited stdin.
		execFileSync("powershell", ["-NoProfile", "-NonInteractive", "-EncodedCommand", encoded], {
			stdio: ["inherit", "ignore", "ignore"],
		});
		return true;
	} catch {
		return false;
	}
}

export default function (pi: ExtensionAPI) {
	pi.on("session_start", (_event, ctx) => {
		if (ctx.mode !== "tui") return;

		announce({ agent: "pi", model: ctx.model?.id, effort: ctx.thinkingLevel });
		announceCatalog(ctx);

		const options = editorOptions(ctx.cwd);
		ctx.ui.setEditorComponent((tui, theme, keybindings) => new PanoramaEditor(tui, theme, keybindings, options));

		if (process.platform !== "win32" || !usesFullscreen(ctx.cwd)) return;

		// ConPTY only forwards mouse-tracking escapes once the console input handle
		// has ENABLE_MOUSE_INPUT, so the ones pi sent at startup were dropped.
		if (enableConsoleMouse()) process.stdout.write(MOUSE_TRACKING);
	});

	pi.on("model_select", (event) => announce({ model: event.model.id, efforts: efforts(event.model) }));
	pi.on("thinking_level_select", (event) => announce({ effort: event.level }));
}
