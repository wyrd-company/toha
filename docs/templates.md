---
docs: true
title: Authoring templates
order: 5
relationships:
  describes: toha
  references: template-format
---

A template folder has `template.yml` at its root and a `template/` source
subdirectory by default. Toha interviews the caller, then renders file names and
file contents through Jinja. Other files in the folder are support material;
file rules can use them to generate repeated files with `each`.

The
[demo template](https://github.com/wyrd-company/toha/blob/main/docs/examples/demo/template.yml)
has two text questions and one rendered file. The
[basic](https://github.com/wyrd-company/toha/blob/main/docs/examples/basic/template.yml),
[branching](https://github.com/wyrd-company/toha/blob/main/docs/examples/branching/template.yml),
and
[generated files](https://github.com/wyrd-company/toha/blob/main/docs/examples/generated-files/template.yml)
examples show defaults, conditions, data, and repeated files.

Use the
[template format specification](https://github.com/wyrd-company/toha/blob/main/docs/specifications/template-format.yml)
for all fields, types, and validation rules.
