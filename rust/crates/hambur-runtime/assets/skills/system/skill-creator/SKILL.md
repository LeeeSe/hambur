---
name: skill-creator
description: Create or update Hambur skills: reusable task instructions stored under /var/hambur/skills, with lean SKILL.md files and optional references, scripts, templates, and assets.
tags: [skills, authoring, workflow]
---

# Skill Creator

## When To Use

Use this skill when the user wants to create, improve, port, debug, or organize a Hambur skill.

Hambur skills live in `/var/hambur/skills` inside the Linux sandbox. That path is shared by all chat sessions. A skill is a folder containing a required `SKILL.md` and optional supporting folders:

```text
/var/hambur/skills/<skill-name>/
├── SKILL.md
├── references/
├── scripts/
├── templates/
└── assets/
```

## Core Principles

Keep `SKILL.md` lean. The model already knows general programming, writing, and reasoning patterns. Add only task-specific procedures, domain facts, tool usage notes, and guardrails that materially improve future work.

Use progressive disclosure:

1. Frontmatter `name` and `description` are always visible through `skills_list`.
2. `SKILL.md` is loaded only through `skill_view`.
3. Supporting files are loaded only when needed through `skill_view(name, file_path)` or file tools under `/var/hambur/skills`.

Match format to fragility:

- Use plain instructions when judgment and context matter.
- Use checklists or pseudocode when consistency matters but variation is normal.
- Use scripts when the task is repetitive, exact, or easy to get subtly wrong.

Do not create extra docs such as `README.md`, changelogs, installation notes, or marketing copy unless the user specifically asks. Put only agent-useful material in the skill.

## Creation Workflow

1. Clarify the target behavior with concrete examples.
   Ask only for information that cannot be inferred. Useful questions: what tasks should trigger the skill, what output should look like, what tools or files it depends on, and what mistakes the skill should prevent.

2. Choose the location and name.
   Use lowercase letters, digits, and hyphens. Name the folder exactly after the skill name. Prefer short action-oriented names, for example `analyze-logs`, `write-release-notes`, or `android-rootfs-debug`.

3. Decide what belongs in `SKILL.md`.
   Keep the main file focused on when to use the skill, the workflow, important constraints, and how to verify results.

4. Move detailed material out of `SKILL.md`.
   Use:
   - `references/` for schemas, APIs, examples, policies, or long domain notes.
   - `scripts/` for deterministic helpers.
   - `templates/` for reusable output or config templates.
   - `assets/` for files used as inputs or output resources.

5. Write clear frontmatter.
   Required:

   ```yaml
   ---
   name: skill-name
   description: One concise sentence describing what the skill does and when to use it.
   ---
   ```

   The description is the main discovery surface. Make it specific enough that a model can decide when to call `skill_view`.

6. Validate in Hambur.
   Run or ask the model to run `skills_list` and confirm the skill appears with the expected description. Then run `skill_view` and confirm the content is concise, actionable, and references any supporting files by relative path.

## SKILL.md Shape

Prefer this section order:

```markdown
# Skill Name

## When To Use

## Workflow

## Constraints

## Supporting Files

## Verification
```

Omit sections that do not add value. Keep headings stable and plain.

## Supporting File Guidance

If a skill has supporting files, mention them in `SKILL.md` with when to load them:

```markdown
## Supporting Files

- `references/api.md`: load when implementing against the service API.
- `templates/report.md`: use when the user asks for the standard report format.
- `scripts/validate.sh`: run after editing generated files.
```

Avoid deep reference chains. A model should discover all important supporting files directly from `SKILL.md`.

## Quality Bar

A good Hambur skill:

- Is short enough to load without bloating the conversation.
- Teaches non-obvious workflow or domain knowledge.
- Names concrete files, commands, formats, or verification steps when they matter.
- Avoids restating generic assistant behavior.
- Can be used across sessions because it lives under `/var/hambur/skills`.
- Does not rely on slash commands or hidden triggers; discovery happens through `skills_list` and loading through `skill_view`.

## Update Existing Skills

When updating a skill, preserve user-authored intent. Read the existing `SKILL.md`, identify what is outdated or bloated, and patch only what improves future use. If adding a reference, link it from `SKILL.md` so it is discoverable.
