import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import "@mantine/core/styles.css";
import "@mantine/notifications/styles.css";

async function bootstrap() {
  if (import.meta.env.DEV && new URLSearchParams(window.location.search).get("ui-fixture") === "1") {
    const { installControlFixture } = await import("./dev-control-fixture");
    installControlFixture();
  }
  createRoot(document.getElementById("root")!).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
}

void bootstrap();
