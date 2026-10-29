import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Overlay from "./Overlay";

// The overlay window loads the same bundle with #overlay in the URL.
const isOverlay = window.location.hash === "#overlay";
if (isOverlay) document.documentElement.classList.add("overlay");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{isOverlay ? <Overlay /> : <App />}</React.StrictMode>,
);
