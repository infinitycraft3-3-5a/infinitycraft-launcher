import { mount } from "svelte";
import App from "./App.svelte";

const root = document.getElementById("app");
if (!root) throw new Error("Missing #app mount node");

const app = mount(App, { target: root });

export default app;
