/** 固定空间槽位决定显示顺序，当前首选空间不参与排序。 */
export function compareWorkspaceOrder(
  left: { id: string; name: string },
  right: { id: string; name: string },
) {
  const slots = ["隔离空间 A", "隔离空间 B", "隔离空间 C", "隔离空间 D", "隔离空间 E"];
  const order = (name: string) => {
    const index = slots.indexOf(name);
    return index < 0 ? slots.length : index;
  };
  return order(left.name) - order(right.name) || left.name.localeCompare(right.name) || left.id.localeCompare(right.id);
}
