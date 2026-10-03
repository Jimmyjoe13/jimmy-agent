// Serveur MCP minimal (stdio, JSON-RPC ligne par ligne) pour les tests.
// Il expose deux outils : `echo` (renvoie le texte) et `files.fail` (nom
// pointé, toujours en erreur) pour vérifier la normalisation des noms et la
// remontée des erreurs `isError`.
const readline = require("readline");

const rl = readline.createInterface({ input: process.stdin });

function send(message) {
  process.stdout.write(JSON.stringify(message) + "\n");
}

rl.on("line", (line) => {
  let request;
  try {
    request = JSON.parse(line);
  } catch {
    return;
  }
  const { id, method, params } = request;
  if (id === undefined) return; // notification : pas de réponse

  // Une notification parasite avant la réponse : le client doit l'ignorer.
  send({ jsonrpc: "2.0", method: "notifications/message", params: { level: "info" } });

  switch (method) {
    case "initialize":
      send({
        jsonrpc: "2.0",
        id,
        result: {
          protocolVersion: "2024-11-05",
          capabilities: { tools: {} },
          serverInfo: { name: "echo", version: "1.0.0" },
        },
      });
      break;
    case "tools/list":
      send({
        jsonrpc: "2.0",
        id,
        result: {
          tools: [
            {
              name: "echo",
              description: "Renvoie le texte reçu.",
              inputSchema: { type: "object", properties: { text: { type: "string" } }, required: ["text"] },
            },
            {
              name: "files.fail",
              description: "Échoue toujours.",
              inputSchema: { type: "object", properties: {} },
            },
          ],
        },
      });
      break;
    case "tools/call":
      if (params.name === "echo") {
        send({ jsonrpc: "2.0", id, result: { content: [{ type: "text", text: `écho : ${params.arguments.text}` }] } });
      } else {
        send({ jsonrpc: "2.0", id, result: { isError: true, content: [{ type: "text", text: "échec volontaire" }] } });
      }
      break;
    default:
      send({ jsonrpc: "2.0", id, error: { code: -32601, message: `méthode inconnue : ${method}` } });
  }
});
