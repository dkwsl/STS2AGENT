# Knowledge Base Plan

The current knowledge base has two layers:

- Static index layer
  - Directly generated from `extraction/decompiled`
  - Solves "what is this id or internal name"
- Decision support layer
  - Tells MCP agents how to use the indexes
  - Avoids mixing static facts with live game state

## Current Files

- [README.md](/Users/chart/Documents/project/sp/docs/game-knowledge/README.md)
- [agent-reference.md](/Users/chart/Documents/project/sp/docs/game-knowledge/agent-reference.md)
- [playbook.md](/Users/chart/Documents/project/sp/docs/game-knowledge/playbook.md)
- Generated indexes:
  - `characters.md`
  - `cards.md`
  - `card-behaviors.md`
  - `monsters.md`
  - `monster-behaviors.md`
  - `potions.md`
  - `potion-behaviors.md`
  - `events.md`

## Regeneration

```powershell
powershell -ExecutionPolicy Bypass -File "C:/Users/chart/Documents/project/sp/scripts/generate-sts2-knowledge.ps1"
```

## Next Steps

1. Add character ownership and more human-readable effect summaries for cards.
2. Add risk tags and choice semantics for events.
3. Add route, rest-site, shop, and potion strategy rules once those MCP actions are fully implemented.
