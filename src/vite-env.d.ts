/// <reference types="vite/client" />

declare module '*.css';

/** 应用版本号，由 vite.config.ts 的 `define` 注入（来源：package.json 的 version 字段）。 */
declare const __APP_VERSION__: string;
