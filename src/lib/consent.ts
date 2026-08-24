// 云端知情同意书版本常量（07-§3 / 04 API §2-3c）。
//
// 与后端 signCloudConsent 的 consent_version 参数一致；同意书文案升级时
// 同步更新此常量与 DB 中已存值（cloud_consent_version）。
export const CLOUD_CONSENT_VERSION = 'v1.0';
