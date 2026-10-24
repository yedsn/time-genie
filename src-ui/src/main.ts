import { createApp } from "vue";
import { createPinia } from "pinia";
import ElementPlus from "element-plus";
import "element-plus/dist/index.css";
import App from "./App.vue";
import "./styles.css";
import { hoverSwitchDirective } from "./services/hoverNavigation";

createApp(App).use(createPinia()).use(ElementPlus).directive("hover-switch", hoverSwitchDirective).mount("#app");
