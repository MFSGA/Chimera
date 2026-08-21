const merge = `# Clash Chimera Merge Template (YAML)
#
# This transform is applied after the selected profile is loaded.
# YAML mappings are merged recursively. Scalars and lists replace existing values.
#
# Example:
# dns:
#   enable: true
`;

const javascript = `// Clash Chimera JavaScript Transform Template

/** @type {config} */
export default function (profile) {
  return profile;
}
`;

const luascript = `-- Clash Chimera Lua Transform Template

return config;
`;

const profile = `# Clash Chimera Profile Template
#
# Fill your local profile content here.

proxies:

proxy-groups:

rules:
`;

export const ProfileTemplate = {
  merge,
  javascript,
  luascript,
  profile,
} as const;
