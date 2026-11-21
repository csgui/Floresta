# Dossel chat

Dossel can use a local Ollama model to answer questions about the running node.
The model uses read-only tools for block height, blockchain status, network
information, and peer information. Conversations are kept in the current chat
session and sent to Ollama on the local machine.

1. Install and start Ollama, then pull a model that supports tool calling:

   ```sh
   ollama pull qwen3
   ```

2. Start Florestad with Dossel enabled. To use a different installed model,
   set `DOSSEL_OLLAMA_MODEL` in the environment of the Florestad process.

   ```sh
   DOSSEL_OLLAMA_MODEL=qwen3 cargo run -p florestad --features dossel -- --dossel
   ```

3. In another terminal, connect to Dossel:

   ```sh
   ./contrib/floresta-repl
   ```

4. Enter `(chat)` and ask, for example, `What is the current block height?`.
   Type `:q` to return to the Scheme REPL. Starting `(chat)` again starts a
   fresh conversation.

Ollama must listen on `127.0.0.1:11434`. If it is unavailable or the model has
not been pulled, the chat reports the error and allows you to try again. Some
models do not support tool calling; use one that does for live node answers.
The node data returned by the tools is sent to the local Ollama process.

The model can make mistakes. For an exact value, also use Dossel's existing
`(get-block-height)` or `(rpc-call "getblockchaininfo")` procedures.
