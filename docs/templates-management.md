---
docs: true
title: Managing templates
order: 6
relationships:
  describes: toha
  references:
    - command-line-interface
    - template-registry
---

Install a Git template by address. A repository can hold several templates;
`#path` selects one folder. A branch name can follow `@`.

```sh
toha templates add gh:example/templates#notes
toha templates list
toha templates alias notes writing-notes
toha apply writing-notes ./notes
```

The formal name is the source address. The short name comes from `template.yml`;
aliases are names you choose. If several installed templates share a short name,
Toha lists their formal names and alias commands. Use an alias or formal name to
select one.

Hooks need trust. `toha templates add <address> --trust` records trust in the
user registry; `toha apply <template> <path> --trust` grants it for one run.
Update branch-following Git templates with `toha templates update` or
`toha templates update <template>`. Templates pinned to a tag or commit stay at
that revision. See the
[CLI](https://github.com/wyrd-company/toha/blob/main/docs/specifications/command-line-interface.yml)
and
[registry](https://github.com/wyrd-company/toha/blob/main/docs/specifications/template-registry.yml)
specifications.
