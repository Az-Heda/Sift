import type { ReactNode } from "react";

export function Screen({
  title,
  subtitle,
  children,
}: {
  title: string;
  subtitle: string;
  children?: ReactNode;
}) {
  return (
    <section className="screen">
      <header className="screen-head">
        <h1>{title}</h1>
        <p>{subtitle}</p>
      </header>
      {children}
    </section>
  );
}

export function Empty({ text }: { text: string }) {
  return <div className="empty">{text}</div>;
}
