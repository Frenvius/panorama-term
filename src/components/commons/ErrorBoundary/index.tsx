import React from 'react';

interface ErrorBoundaryProps {
  children: React.ReactNode;
  fallback: (retry: () => void) => React.ReactNode;
}

interface ErrorBoundaryState {
  failed: boolean;
}

class ErrorBoundary extends React.Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { failed: false };

  static getDerivedStateFromError = (): ErrorBoundaryState => ({ failed: true });

  retry = () => this.setState({ failed: false });

  render = () => (this.state.failed ? this.props.fallback(this.retry) : this.props.children);
}

export default ErrorBoundary;
