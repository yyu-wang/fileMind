import { describe, it, expect, vi } from 'vitest';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { CloudProviderFormCard } from './CloudProviderFormCard';

describe('CloudProviderFormCard', () => {
  it('should render empty form for create mode', () => {
    render(<CloudProviderFormCard onSubmit={() => undefined} />);
    expect(screen.getByText('添加云提供商')).toBeInTheDocument();
    expect(screen.getByPlaceholderText('如 qwen / moonshot / glm')).toBeInTheDocument();
  });

  it('should block submit with invalid slug and show error', async () => {
    const onSubmit = vi.fn();
    render(<CloudProviderFormCard onSubmit={onSubmit} />);
    const submit = screen.getByText('添加提供商');
    fireEvent.click(submit);
    await waitFor(() => {
      expect(screen.getByText(/标识仅允许小写字母/)).toBeInTheDocument();
    });
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it('should allow submitting a well-formed create payload', async () => {
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    render(<CloudProviderFormCard onSubmit={onSubmit} />);
    fireEvent.change(screen.getByPlaceholderText('如 qwen / moonshot / glm'), {
      target: { value: 'qwen' },
    });
    fireEvent.change(screen.getByPlaceholderText('如 DashScope / Claude API'), {
      target: { value: 'DashScope' },
    });
    fireEvent.change(screen.getByLabelText('Base URL'), {
      target: { value: 'https://dashscope.aliyuncs.com/compatible-mode/v1' },
    });
    fireEvent.click(screen.getByText('添加提供商'));
    await waitFor(() => {
      expect(onSubmit).toHaveBeenCalledWith({
        provider_key: 'qwen',
        name: 'DashScope',
        remark: null,
        website: null,
        base_url: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
      });
    });
  });

  it('should reject base_url ending with slash', async () => {
    const onSubmit = vi.fn();
    render(<CloudProviderFormCard onSubmit={onSubmit} />);
    const baseUrl = screen.getByLabelText('Base URL');
    fireEvent.change(baseUrl, { target: { value: 'https://example.com/v1/' } });
    fireEvent.blur(baseUrl);
    await waitFor(() => {
      expect(screen.getByText(/不能以「\/」结尾/)).toBeInTheDocument();
    });
  });

  it('should render edit mode with initial values and disabled slug input', () => {
    render(
      <CloudProviderFormCard
        initial={{
          id: '1',
          provider_key: 'qwen',
          name: 'DashScope',
          remark: '公司',
          website: 'https://dashscope.aliyun.com',
          base_url: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
          is_builtin: false,
          created_at: '2026-01-01T00:00:00Z',
          updated_at: '2026-01-01T00:00:00Z',
        }}
        onSubmit={() => undefined}
      />,
    );
    const slugInput = screen.getByPlaceholderText('如 qwen / moonshot / glm') as HTMLInputElement;
    expect(slugInput.disabled).toBe(true);
    expect(slugInput.value).toBe('qwen');
    expect(screen.getByLabelText('提供商显示名称')).toHaveValue('DashScope');
  });
});
