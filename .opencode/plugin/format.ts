import type { Plugin } from "@opencode-ai/plugin";

interface FormatterOptions {
	args?: string[];
	exts: string[];
}

const formatters: {
	[key: string]: FormatterOptions;
} = {
	cargo: {
		args: ["fmt", "--"],
		exts: [".rs"],
	},
	taplo: {
		args: ["fmt"],
		exts: [".toml"],
	},
	stylua: {
		exts: [".lua"],
	},
	"markdownlint-cli2": {
		args: ["--fix"],
		exts: [".md"],
	},
};

export const FormatPlugin: Plugin = async ({ $, client }) => {
	return {
		event: async ({ event }) => {
			if (event.type === "file.edited") {
				const file = event.properties.file;
				const extension = file.substring(file.lastIndexOf("."));

				const formatter = Object.entries(formatters).find(([_, opts]) =>
					opts.exts.includes(extension),
				);
				if (!formatter) {
					return;
				}

				const [command, opts] = formatter;
				const result = await $`${command} ${[...(opts.args || []), file]}`;
				if (result && result.exitCode !== 0) {
					await client.tui.showToast({
						body: {
							message: `Failed to format file "${file}":\n${result.stderr}`,
							variant: "error",
						},
					});
				}
			}
		},
	};
};
