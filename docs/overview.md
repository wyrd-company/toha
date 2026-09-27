---
docs: true
title: Toha
order: 1
relationships:
  describes: toha
---

Toha generates projects and files from templates. A template asks questions,
then renders file paths and contents with Jinja. You can preview the plan before
Toha writes anything.

![Toha terminal demo](assets/demo.gif)

- Run the same interview from a terminal, a script, an agent, or the Rust crate.
- Resume an interview later, or supply answers from a JSON file.
- Render paths and files with typed Jinja values, conditions, and loops.
- Copy static files and generate one file for each item in a list.
- Run template hooks only when you trust the template.
- Use local folders or Git repositories, with short names and aliases for
  installed templates.
- Read the built-in agent guidance with `toha skills list` and
  `toha skills view`.

Start with [Getting started](/docs/toha/getting-started). To make your own
template, read [Authoring templates](/docs/toha/templates).
