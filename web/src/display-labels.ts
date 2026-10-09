import { t } from "@xcss/admin-ui/i18n";
const labels: Record<string, readonly [string, string]> = {
  "hardware.memory": ["内存模块识别", "Memory module inventory"], "hardware.thunderbolt": ["雷电 / USB4 识别", "Thunderbolt / USB4 inventory"],
  "hardware.monitors": ["显示器识别", "Monitor inventory"], "hardware.bluetooth": ["蓝牙识别", "Bluetooth inventory"],
  "hardware.usb_controllers": ["USB 控制器识别", "USB controller inventory"], "hardware.usb_devices": ["USB 设备识别", "USB device inventory"], "hardware.audio": ["音频设备识别", "Audio device inventory"],
  id: ["标识", "ID"], name: ["名称", "Name"], os: ["操作系统", "Operating system"], arch: ["架构", "Architecture"],
  registered_at: ["注册时间", "Registered at"], last_seen_at: ["最近连接", "Last connection"], latest_collected_at: ["最近上报", "Last report"],
  status: ["状态", "Status"], capabilities: ["采集能力", "Collection capabilities"],
  cpu_usage_percent: ["处理器使用率（%）", "CPU usage (%)"], memory_usage_percent: ["内存使用率（%）", "Memory usage (%)"],
  network_received_bytes_per_second: ["网络接收（字节/秒）", "Network receive (bytes/s)"], network_transmitted_bytes_per_second: ["网络发送（字节/秒）", "Network transmit (bytes/s)"],
  disk_read_bytes_per_second: ["磁盘读取（字节/秒）", "Disk read (bytes/s)"], disk_written_bytes_per_second: ["磁盘写入（字节/秒）", "Disk write (bytes/s)"],
  max_temperature_celsius: ["最高温度（℃）", "Maximum temperature (°C)"], gpu_utilization_percent: ["显卡使用率（%）", "GPU usage (%)"], gpu_memory_usage_percent: ["显存使用率（%）", "GPU memory usage (%)"],
  online: ["在线", "Online"], stale: ["上报延迟", "Stale"], offline: ["离线", "Offline"], waiting: ["等待确认", "Waiting for confirmation"],
  activated: ["已激活", "Activated"], consumed: ["已使用", "Consumed"], expired: ["已过期", "Expired"], denied: ["已拒绝", "Denied"],
  unsupported: ["不受支持", "Unsupported"], not_present: ["设备不存在", "Not present"], driver_missing: ["缺少驱动", "Driver missing"],
  permission_denied: ["权限不足", "Permission denied"], transient: ["暂时不可用", "Temporarily unavailable"], invalid_data: ["数据无效", "Invalid data"],
  cpu: ["处理器", "CPU"], memory: ["内存", "Memory"], network: ["网络", "Network"], disk: ["磁盘", "Disk"],
  temperature: ["温度", "Temperature"], gpu: ["显卡", "GPU"], nvidia: ["NVIDIA 显卡", "NVIDIA GPU"],
};
export function displayLabel(value: string): string {
  if (value.startsWith("system.")) return displayLabel(value.slice(7));
  if (value.startsWith("gpu.")) {
    const parts = value.split(".");
    const vendors: Record<string, string> = { nvidia: "NVIDIA", amd: "AMD", intel: "Intel", apple: "Apple", windows: "Windows WDDM" };
    const vendor = Object.hasOwn(vendors, parts[1]) ? vendors[parts[1]] : undefined;
    if (vendor) return t("{0} 显卡采集", "{0} GPU collection", [vendor]);
  }
  const label = Object.hasOwn(labels, value) ? labels[value] : undefined;
  return label ? t(...label) : t("未识别的项目", "Unrecognized item");
}
