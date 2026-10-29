// 剪贴板：报告必须能被原样复制进 issue，所以两条路径都要走通。
//
// Tauri 的 WebView 里 `navigator.clipboard` 在部分环境会被拒（非安全上下文 / 权限策略），
// 因此保留 `textarea + execCommand` 兜底。返回 `false` 时由调用方提示「手动全选复制」，
// 不要静默失败——用户会以为已经复制好了。

export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    // 继续走兜底路径。
  }
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "true");
  area.style.position = "fixed";
  area.style.top = "0";
  area.style.left = "0";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  let ok = false;
  try {
    ok = document.execCommand("copy");
  } catch {
    ok = false;
  }
  document.body.removeChild(area);
  return ok;
}
