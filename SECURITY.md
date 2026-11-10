# Security policy

## Reporting a vulnerability

Please report security problems privately through GitHub's [private vulnerability reporting](https://github.com/lvndry/euphoria/security/advisories/new) for this repository, not in a public issue.

Include what you found, how to reproduce it, and what an attacker could do with it. You will get a reply within a week.

## Scope

Euphoria reads audio files and writes tags to them. Problems worth reporting include:

- a crafted audio file that crashes Euphoria, corrupts other files, or runs code,
- a tag write that damages audio data or touches files outside what the user selected,
- any network access, since Euphoria is meant to work fully offline.

## Supported versions

Only the latest release receives security fixes.
