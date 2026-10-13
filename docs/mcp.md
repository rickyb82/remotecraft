# Server MCP

> **Ultimo aggiornamento:** 2026-10-13

`remotecraft-cli mcp` è un server Model Context Protocol (JSON-RPC su stdio). Con un assistente
AI collegato puoi dire, per esempio:

- *«di chi è il PC 123 456 789?»*
- *«avvia un intervento remoto sul PC reception dello Studio Rossi»*
- *«ho finito: ho reinstallato il driver della stampante»*
- *«registra l'intervento di ieri dalle 15 alle 16:30 da Corvo, in sede, cablaggio»*
- *«ore di ottobre per cliente»*
- *«lo Studio Rossi ha firmato informativa e consenso all'assistenza remota, modulo 2026/12»*
- *«cosa manca nel registro GDPR?»*

Ogni strumento è un comando del motore: l'assistente può fare quello che fa l'API, niente di
più. Agli strumenti arrivano solo i parametri dichiarati nel loro schema.

## Collegarlo

**Sul server, per un tecnico** (consigliato: un solo gestionale, scritture tramite il server):

```sh
# il framework crea il token del tecnico
curl -X POST -H "Authorization: Bearer $REMOTECRAFT_TOKEN" http://127.0.0.1:7979/api/users/marco/token
# il processo MCP del tecnico
remotecraft-cli mcp --server https://assistenza.example.it --token <token di marco>
```

**Claude Code**:

```sh
claude mcp add remotecraft -- remotecraft-cli mcp --server https://assistenza.example.it --token <token>
```

**Claude Desktop** (`claude_desktop_config.json`):

```json
{"mcpServers": {"remotecraft": {"command": "remotecraft-cli", "args": ["mcp", "--server", "https://assistenza.example.it", "--token", "<token>"]}}}
```

In locale, su un gestionale nella propria cartella: `remotecraft-cli mcp --as riccardo`.
Per provarlo: `remotecraft-cli mcp --demo`.

## Strumenti

| Strumento | Comando |
|---|---|
| `find_customers`, `get_customer`, `add_customer`, `update_customer`, `archive_customer` | `customer.*` |
| `add_device`, `update_device`, `find_device`, `list_devices`, `connect_device` | `device.*` |
| `start_intervention`, `stop_intervention`, `log_intervention`, `update_intervention`, `delete_intervention`, `list_interventions`, `running_interventions` | `intervention.*` |
| `time_report` | `report.time` |
| `privacy_status`, `privacy_history`, `record_notice_delivery`, `record_consent`, `withdraw_consent` | `privacy.*` |
| `list_notices`, `get_notice`, `add_notice` | `notice.*` |
| `gdpr_export`, `gdpr_erase`, `gdpr_review`, `audit_log` | `gdpr.*`, `audit.list` |
| `rustdesk_setup`, `check_rustdesk_server` | `rustdesk.*` |
| `get_preferences`, `set_preferences`, `status`, `list_commands`, `run_command` | `prefs.*`, `app.*` |

Prompt: `new_customer` (cliente da una descrizione: anagrafica, PC, informativa e consensi),
`log_work` (intervento da una nota), `monthly_report` (ore del mese pronte da fatturare),
`privacy_check` (revisione del registro GDPR). Risorse: `remotecraft://running` (cronometri
attivi) e `remotecraft://review` (cosa manca nel registro).

## Regole che l'assistente riceve

- Non inventare consensi, firmatari o prove: chiedili.
- Quando l'avvio remoto viene rifiutato per mancanza di consenso, spiegarlo e registrare il
  consenso, oppure, solo se la persona lo dice, usare `consent_override` con il motivo (che resta
  nell'intervento e nell'audit).
- Prima di `gdpr_erase`, `delete_intervention` o `archive_customer`, mostrare cosa succede e
  aspettare un sì.
- L'export GDPR contiene dati personali: consegnarlo solo a chi ne ha diritto.
